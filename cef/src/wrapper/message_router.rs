use super::{browser_info_map::*, message_router_utils as mru};
use crate::*;
use std::{
    collections::{BTreeMap, VecDeque},
    ops::{AddAssign, ControlFlow, Range},
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

/// Used to configure the query router. The same values must be passed to both
/// [MessageRouterBrowserSide] and [MessageRouterRendererSide]. If using
/// multiple router pairs make sure to choose values that do not conflict.
#[derive(Clone, Debug)]
pub struct MessageRouterConfig {
    /// Name of the JavaScript function that will be added to the 'window' object
    /// for sending a query. The default value is "cefQuery".
    pub js_query_function: String,
    /// Name of the JavaScript function that will be added to the 'window' object
    /// for canceling a pending query. The default value is "cefQueryCancel".
    pub js_cancel_function: String,
    /// Messages of size (in bytes) larger than this threshold will be sent via
    /// shared memory region.
    pub message_size_threshold: usize,
}

impl MessageRouterConfig {
    pub const fn validate(&self) -> bool {
        !self.js_query_function.is_empty() && !self.js_cancel_function.is_empty()
    }
}

impl Default for MessageRouterConfig {
    fn default() -> Self {
        Self {
            js_query_function: "cefQuery".to_string(),
            js_cancel_function: "cefQueryCancel".to_string(),
            message_size_threshold: RESPONSE_SIZE_THRESHOLD,
        }
    }
}

/// This trait acts as a container for managing binary data. It retains
/// references to the underlying backing store, ensuring it is valid as long as
/// the BinaryBuffer exists. This allows efficient, zero-copy access to data
/// received from another process.
pub trait BinaryBuffer: Send {
    /// Returns the read-only pointer to the memory.
    fn data(&self) -> &[u8];
    /// Returns the writable pointer to the memory.
    fn data_mut(&mut self) -> &mut [u8];
}

/// Callback associated with a single pending asynchronous query. Execute the
/// Success or Failure method to send an asynchronous response to the
/// associated JavaScript handler. It is a runtime error to destroy a Callback
/// object associated with an uncanceled query without first executing one of
/// the callback methods. The methods of this class may be called on any
/// browser process thread.
pub trait BrowserSideCallback: Send + Sync {
    /// Notify the associated JavaScript onSuccess callback that the query has
    /// completed successfully with the specified string |response|.
    fn success_str(&self, response: &str);
    /// Notify the associated JavaScript onSuccess callback that the query has
    /// completed successfully with binary data.
    fn success_binary(&self, data: &[u8]);
    /// Notify the associated JavaScript onFailure callback that the query has
    /// failed with the specified |error_code| and |error_message|.
    fn failure(&self, error_code: i32, error_message: &str);
}

/// Implement this interface to handle queries. All methods will be executed
/// on the browser process UI thread.
pub trait BrowserSideHandler: Send + Sync {
    /// Executed when a new query is received. |query_id| uniquely identifies
    /// the query for the life span of the router. Return true to handle the
    /// query or false to propagate the query to other registered handlers, if
    /// any. If no handlers return true from this method then the query will be
    /// automatically canceled with an error code of -1 delivered to the
    /// JavaScript onFailure callback. If this method returns true then a
    /// Callback method must be executed either in this method or asynchronously
    /// to complete the query.
    fn on_query_str(
        &self,
        _browser: Option<Browser>,
        _frame: Option<Frame>,
        _query_id: i64,
        _request: &str,
        _persistent: bool,
        _callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        false
    }

    /// Executed when a new query is received. |query_id| uniquely identifies
    /// the query for the life span of the router. Return true to handle the
    /// query or false to propagate the query to other registered handlers, if
    /// any. If no handlers return true from this method then the query will be
    /// automatically canceled with an error code of -1 delivered to the
    /// JavaScript onFailure callback. If this method returns true then a
    /// Callback method must be executed either in this method or asynchronously
    /// to complete the query.
    fn on_query_binary(
        &self,
        _browser: Option<Browser>,
        _frame: Option<Frame>,
        _query_id: i64,
        _request: &dyn BinaryBuffer,
        _persistent: bool,
        _callback: Arc<Mutex<dyn BrowserSideCallback>>,
    ) -> bool {
        false
    }

    /// Executed when a query has been canceled either explicitly using the
    /// JavaScript cancel function or implicitly due to browser destruction,
    /// navigation or renderer process termination. It will only be called for
    /// the single handler that returned true from OnQuery for the same
    /// |query_id|. No references to the associated Callback object should be
    /// kept after this method is called, nor should any Callback methods be
    /// executed.
    fn on_query_canceled(&self, _browser: Option<Browser>, _frame: Option<Frame>, _query_id: i64) {}
}

/// Implements the browser side of query routing. The methods of this trait may
/// be called on any browser process thread unless otherwise indicated.
pub trait MessageRouterBrowserSide {
    type Callback: BrowserSideCallback;

    /// Create a new router with the specified configuration.
    fn new(config: MessageRouterConfig) -> Arc<Self>;

    /// Add a new query handler. If |first| is true it will be added as the first
    /// handler, otherwise it will be added as the last handler. Must be called on
    /// the browser process UI thread.
    fn add_handler(&self, handler: Arc<dyn BrowserSideHandler>, first: bool) -> Option<HandlerId>;

    /// Remove an existing query handler. Any pending queries associated with the
    /// handler will be canceled. Handler::OnQueryCanceled will be called and the
    /// associated JavaScript onFailure callback will be executed with an error
    /// code of -1. Returns true if the handler is removed successfully or false
    /// if the handler is not found. Must be called on the browser process UI
    /// thread.
    fn remove_handler(&self, handler_id: HandlerId) -> bool;

    /// Cancel all pending queries associated with either |browser| or |handler|.
    /// If both |browser| and |handler| are NULL all pending queries will be
    /// canceled. Handler::OnQueryCanceled will be called and the associated
    /// JavaScript onFailure callback will be executed in all cases with an error
    /// code of -1.
    fn cancel_pending(&self, browser: Option<Browser>, handler_id: Option<HandlerId>);

    /// Returns the number of queries currently pending for the specified
    /// |browser| and/or |handler|. Either or both values may be empty. Must be
    /// called on the browser process UI thread.
    fn pending_count(&self, browser: Option<Browser>, handler_id: Option<HandlerId>) -> usize;
}

/// The below methods should be called from other CEF handlers. They must be
/// called exactly as documented for the router to function correctly.
pub trait MessageRouterBrowserSideHandlerCallbacks: MessageRouterBrowserSide {
    /// Call from CefLifeSpanHandler::OnBeforeClose. Any pending queries
    /// associated with |browser| will be canceled and Handler::OnQueryCanceled
    /// will be called. No JavaScript callbacks will be executed since this
    /// indicates destruction of the browser.
    fn on_before_close(&self, browser: Option<Browser>);

    /// Call from CefRequestHandler::OnRenderProcessTerminated. Any pending
    /// queries associated with |browser| will be canceled and
    /// Handler::OnQueryCanceled will be called. No JavaScript callbacks will be
    /// executed since this indicates destruction of the context.
    fn on_render_process_terminated(&self, browser: Option<Browser>);

    /// Call from CefRequestHandler::OnBeforeBrowse only if the navigation is
    /// allowed to proceed. If |frame| is the main frame then any pending queries
    /// associated with |browser| will be canceled and Handler::OnQueryCanceled
    /// will be called. No JavaScript callbacks will be executed since this
    /// indicates destruction of the context.
    fn on_before_browse(&self, browser: Option<Browser>, frame: Option<Frame>);

    /// Call from CefClient::OnProcessMessageReceived. Returns true if the message
    /// is handled by this router or false otherwise.
    fn on_process_message_received(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        source_process: ProcessId,
        message: Option<ProcessMessage>,
    ) -> bool;
}

/// Implements the renderer side of query routing. The methods of this class
/// must be called on the render process main thread.
pub trait MessageRouterRendererSide {
    /// Create a new router with the specified configuration.
    fn new(config: MessageRouterConfig) -> Arc<Self>;

    /// Returns the number of queries currently pending for the specified
    /// |browser| and/or |context|. Either or both values may be empty.
    fn pending_count(&self, browser: Option<Browser>, context: Option<V8Context>) -> usize;
}

/// The below methods should be called from other CEF handlers. They must be
/// called exactly as documented for the router to function correctly.
pub trait MessageRouterRendererSideHandlerCallbacks: MessageRouterRendererSide {
    /// Call from CefRenderProcessHandler::OnContextCreated. Registers the
    /// JavaScripts functions with the new context.
    fn on_context_created(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        context: Option<V8Context>,
    );

    /// Call from CefRenderProcessHandler::OnContextReleased. Any pending queries
    /// associated with the released context will be canceled and
    /// Handler::OnQueryCanceled will be called in the browser process.
    fn on_context_released(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        context: Option<V8Context>,
    );

    /// Call from CefRenderProcessHandler::OnProcessMessageReceived. Returns true
    /// if the message is handled by this router or false otherwise.
    fn on_process_message_received(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        source_process: Option<ProcessId>,
        message: Option<ProcessMessage>,
    ) -> bool;
}

/// ID value reserved for internal use.
const RESERVED_ID: i32 = 0;

/// Appended to the JS function name for related IPC messages.
const MESSAGE_SUFFIX: &str = "Msg";

/// JS object member argument names for cefQuery.
struct ObjectMember;

impl ObjectMember {
    const REQUEST: &str = "request";
    const ON_SUCCESS: &str = "onSuccess";
    const ON_FAILURE: &str = "onFailure";
    const PERSISTENT: &str = "persistent";
}

/// Default error information when a query is canceled.
struct CanceledError;

impl CanceledError {
    const CODE: i32 = -1;
    const MESSAGE: &str = "The query has been canceled";
}

/// Value of 16KB is chosen as a result of performance tests available at
/// http://tests/ipc_performance
const RESPONSE_SIZE_THRESHOLD: usize = 16 * 1024;

/// A helper template for generating ID values.
struct IdGenerator<T>
where
    T: Copy + AddAssign + Ord + From<i32>,
{
    next_id: T,
    range: Range<T>,
}

impl<T> IdGenerator<T>
where
    T: Copy + AddAssign + Ord + From<i32>,
{
    fn new(max: T) -> Self {
        let next_id = RESERVED_ID.into();
        Self {
            next_id,
            range: next_id..max,
        }
    }

    fn next(&mut self) -> T {
        if self.next_id >= self.range.end {
            self.next_id = self.range.start;
        }

        self.next_id += 1_i32.into();
        self.next_id
    }
}

pub type HandlerId = i32;

pub struct BrowserSideRouterCallback {
    weak_callback: Weak<Mutex<Self>>,
    router: Option<Arc<BrowserSideRouter>>,
    browser_id: i32,
    query_id: i64,
    persistent: bool,
    message_size_threshold: usize,
    query_message_name: String,
}

impl BrowserSideRouterCallback {
    fn new(
        router: Arc<BrowserSideRouter>,
        browser_id: i32,
        query_id: i64,
        persistent: bool,
        message_size_threshold: usize,
        query_message_name: String,
    ) -> Arc<Mutex<Self>> {
        Arc::new_cyclic(|weak_self| {
            Mutex::new(Self {
                weak_callback: weak_self.clone(),
                router: Some(router),
                browser_id,
                query_id,
                persistent,
                message_size_threshold,
                query_message_name,
            })
        })
    }

    fn detach(&mut self) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        self.router = None;
    }
}

impl BrowserSideCallback for BrowserSideRouterCallback {
    fn success_str(&self, response: &str) {
        let builder = mru::create_browser_response_builder(
            self.message_size_threshold,
            &self.query_message_name,
            mru::MessagePayload::from(response),
        );

        // We need to post task here for two reasons:
        // 1) To safely access member variables.
        // 2) To let the router to persist the query information before
        // the Success callback is executed.
        let mut task = BrowserSideRouterCallbackSuccess::new(self.weak_callback.clone(), builder);
        post_task(ThreadId::UI, Some(&mut task));
    }

    fn success_binary(&self, data: &[u8]) {
        let builder = mru::create_browser_response_builder(
            self.message_size_threshold,
            &self.query_message_name,
            mru::MessagePayload::from(data),
        );

        // We need to post task here for two reasons:
        // 1) To safely access member variables.
        // 2) To let the router to persist the query information before
        // the Success callback is executed.
        let mut task = BrowserSideRouterCallbackSuccess::new(self.weak_callback.clone(), builder);
        post_task(ThreadId::UI, Some(&mut task));
    }

    fn failure(&self, error_code: i32, error_message: &str) {
        // We need to post task here for two reasons:
        // 1) To safely access member variables.
        // 2) To give previosly submitted tasks by the Success calls to execute
        // before we invalidate the callback.
        let mut task = BrowserSideRouterCallbackFailure::new(
            self.weak_callback.clone(),
            error_code,
            error_message.to_string(),
        );
        post_task(ThreadId::UI, Some(&mut task));
    }
}

wrap_task! {
    struct BrowserSideRouterCallbackSuccess {
        weak_callback: Weak<Mutex<BrowserSideRouterCallback>>,
        builder: Rc<dyn mru::ProcessMessageBuilder>,
    }

    impl Task {
        fn execute(&self) {
            let Some(callback) = self.weak_callback.upgrade() else {
                return;
            };
            let Ok(mut callback) = callback.lock() else {
                return;
            };

            let router = if callback.persistent {
                callback.router.clone()
            } else {
                // Non-persistent callbacks are only good for a single use.
                callback.router.take()
            };
            let Some(router) = router else {
                return;
            };

            router.on_callback_success(callback.browser_id, callback.query_id, &*self.builder);
        }
    }
}

wrap_task! {
    struct BrowserSideRouterCallbackFailure {
        weak_callback: Weak<Mutex<BrowserSideRouterCallback>>,
        error_code: i32,
        error_message: String,
    }

    impl Task {
        fn execute(&self) {
            let Some(callback) = self.weak_callback.upgrade() else {
                return;
            };
            let Ok(mut callback) = callback.lock() else {
                return;
            };

            // Failure always invalidates the callback.
            let router = callback.router.take();
            let Some(router) = router else {
                return;
            };

            router.on_callback_failure(callback.browser_id, callback.query_id, self.error_code, &self.error_message);
        }
    }
}

/// Structure representing a pending query.
#[derive(Clone, Default)]
struct BrowserSideQueryInfo {
    // Browser and frame originated the query.
    browser: Option<Browser>,
    frame: Option<Frame>,
    // IDs that uniquely identify the query in the renderer process. These
    // values are opaque to the browser process but must be returned with the
    // response.
    context_id: i32,
    request_id: i32,
    // True if the query is persistent.
    is_persistent: bool,
    // Callback associated with the query that must be detached when the query
    // is canceled.
    callback: Option<Arc<Mutex<BrowserSideRouterCallback>>>,
    // Handler that should be notified if the query is automatically canceled.
    handler: Option<(HandlerId, Arc<dyn BrowserSideHandler>)>,
}

#[derive(Default, Clone)]
struct HandlersDeque {
    offset: HandlerId,
    handlers: VecDeque<Option<Arc<dyn BrowserSideHandler>>>,
}

pub struct BrowserSideRouter {
    weak_self: Weak<Self>,
    config: MessageRouterConfig,
    query_message_name: String,
    cancel_message_name: String,
    query_id_generator: Mutex<IdGenerator<i64>>,
    handlers: Mutex<HandlersDeque>,
    browser_query_info_map: Mutex<BrowserInfoMap<i64, BrowserSideQueryInfo>>,
}

impl BrowserSideRouter {
    /// Retrieve a QueryInfo object from the map based on the browser-side query
    /// ID. If |always_remove| is true then the QueryInfo object will always be
    /// removed from the map. Othewise, the QueryInfo object will only be removed
    /// if the query is non-persistent. If |removed| is true the caller is
    /// responsible for deleting the returned QueryInfo object.
    fn get_query_info(
        &self,
        browser_id: i32,
        query_id: i64,
        always_remove: bool,
    ) -> Option<BrowserSideQueryInfo> {
        let Ok(mut browser_query_info_map) = self.browser_query_info_map.lock() else {
            return None;
        };

        struct Visitor {
            always_remove: bool,
        }

        impl BrowserInfoMapVisitor<i64, BrowserSideQueryInfo> for Visitor {
            fn on_next_info(
                &self,
                _browser_id: i32,
                _key: i64,
                value: &BrowserSideQueryInfo,
            ) -> ControlFlow<BrowserInfoMapVisitorResult, BrowserInfoMapVisitorResult> {
                ControlFlow::Continue(if self.always_remove || !value.is_persistent {
                    BrowserInfoMapVisitorResult::RemoveEntry
                } else {
                    BrowserInfoMapVisitorResult::KeepEntry
                })
            }
        }

        let visitor = Visitor { always_remove };
        browser_query_info_map.find(browser_id, query_id, Some(&visitor))
    }

    /// Called by CallbackImpl on success.
    fn on_callback_success(
        &self,
        browser_id: i32,
        query_id: i64,
        builder: &dyn mru::ProcessMessageBuilder,
    ) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Some(info) = self.get_query_info(browser_id, query_id, false) {
            self.send_query_success(
                info.browser,
                info.frame,
                info.context_id,
                info.request_id,
                builder,
            )
        }
    }

    /// Called by CallbackImpl on failure.
    fn on_callback_failure(
        &self,
        browser_id: i32,
        query_id: i64,
        error_code: i32,
        error_message: &str,
    ) {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        if let Some(info) = self.get_query_info(browser_id, query_id, false) {
            self.send_query_failure(
                info.browser,
                info.frame,
                info.context_id,
                info.request_id,
                error_code,
                error_message,
            )
        }
    }

    fn send_query_success(
        &self,
        _browser: Option<Browser>,
        frame: Option<Frame>,
        context_id: i32,
        request_id: i32,
        builder: &dyn mru::ProcessMessageBuilder,
    ) {
        let (Some(frame), Some(mut message)) = (
            frame,
            builder.build_browser_response(context_id, request_id),
        ) else {
            return;
        };

        frame.send_process_message(ProcessId::RENDERER, Some(&mut message));
    }

    fn send_query_failure(
        &self,
        _browser: Option<Browser>,
        frame: Option<Frame>,
        context_id: i32,
        request_id: i32,
        error_code: i32,
        error_message: &str,
    ) {
        let (Some(frame), Some(mut message)) = (
            frame,
            process_message_create(Some(&CefString::from(self.query_message_name.as_str()))),
        ) else {
            return;
        };
        let Some(args) = message.argument_list() else {
            return;
        };

        args.set_int(0, context_id);
        args.set_int(1, request_id);
        args.set_bool(2, 0); // Indicates a failure result.
        args.set_int(3, error_code);
        args.set_string(4, Some(&CefString::from(error_message)));

        frame.send_process_message(ProcessId::RENDERER, Some(&mut message));
    }

    /// Cancel a query that has not been sent to a handler.
    fn cancel_unhandled_query(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        context_id: i32,
        request_id: i32,
    ) {
        self.send_query_failure(
            browser,
            frame,
            context_id,
            request_id,
            CanceledError::CODE,
            CanceledError::MESSAGE,
        );
    }

    /// Cancel a query that has already been sent to a handler.
    fn cancel_query(&self, query_id: i64, query_info: BrowserSideQueryInfo, notify_renderer: bool) {
        if notify_renderer {
            self.send_query_failure(
                query_info.browser.clone(),
                query_info.frame.clone(),
                query_info.context_id,
                query_info.request_id,
                CanceledError::CODE,
                CanceledError::MESSAGE,
            );
        }

        if let Some((_, handler)) = query_info.handler {
            handler.on_query_canceled(query_info.browser, query_info.frame, query_id);
        }

        // Invalidate the callback.
        if let Some(callback) = query_info.callback {
            if let Ok(mut callback) = callback.lock() {
                callback.detach();
            }
        }
    }

    /// Cancel all pending queries associated with either |browser| or |handler|.
    /// If both |browser| and |handler| are NULL all pending queries will be
    /// canceled. Set |notify_renderer| to true if the renderer should be notified.
    fn cancel_pending_for(
        &self,
        browser: Option<Browser>,
        handler: Option<(HandlerId, Arc<dyn BrowserSideHandler>)>,
        notify_renderer: bool,
    ) {
        if currently_on(ThreadId::UI) == 0 {
            let mut task = BrowserSideRouterCancelPendingFor::new(
                self.weak_self.clone(),
                browser,
                handler,
                notify_renderer,
            );
            post_task(ThreadId::UI, Some(&mut task));
            return;
        }

        let (Some(router), Ok(mut browser_query_info_map)) =
            (self.weak_self.upgrade(), self.browser_query_info_map.lock())
        else {
            return;
        };

        if browser_query_info_map.is_empty() {
            return;
        }

        struct Visitor {
            router: Arc<BrowserSideRouter>,
            handler_id: Option<HandlerId>,
            notify_renderer: bool,
        }

        impl BrowserInfoMapVisitor<i64, BrowserSideQueryInfo> for Visitor {
            fn on_next_info(
                &self,
                _browser_id: i32,
                key: i64,
                value: &BrowserSideQueryInfo,
            ) -> ControlFlow<BrowserInfoMapVisitorResult, BrowserInfoMapVisitorResult> {
                ControlFlow::Continue(match (self.handler_id, &value.handler) {
                    (Some(handler_id), Some(entry)) if handler_id != entry.0 => {
                        BrowserInfoMapVisitorResult::KeepEntry
                    }
                    _ => {
                        self.router
                            .cancel_query(key, value.clone(), self.notify_renderer);
                        BrowserInfoMapVisitorResult::RemoveEntry
                    }
                })
            }
        }

        let visitor = Visitor {
            router,
            handler_id: handler.map(|handler| handler.0),
            notify_renderer,
        };

        if let Some(browser) = browser {
            // Cancel all queries associated with the specified browser.
            browser_query_info_map.find_browser_all(browser.identifier(), &visitor);
        } else {
            // Cancel all queries for all browsers.
            browser_query_info_map.find_all(&visitor);
        }
    }

    /// Cancel a query based on the renderer-side IDs. If |request_id| is
    /// kReservedId all requests associated with |context_id| will be canceled.
    fn cancel_pending_request(&self, browser_id: i32, context_id: i32, request_id: i32) {
        let (Some(router), Ok(mut browser_query_info_map)) =
            (self.weak_self.upgrade(), self.browser_query_info_map.lock())
        else {
            return;
        };

        struct Visitor {
            router: Arc<BrowserSideRouter>,
            context_id: i32,
            request_id: i32,
        }

        impl BrowserInfoMapVisitor<i64, BrowserSideQueryInfo> for Visitor {
            fn on_next_info(
                &self,
                _browser_id: i32,
                key: i64,
                value: &BrowserSideQueryInfo,
            ) -> ControlFlow<BrowserInfoMapVisitorResult, BrowserInfoMapVisitorResult> {
                if value.context_id == self.context_id && self.request_id == RESERVED_ID
                    || value.request_id == self.request_id
                {
                    self.router.cancel_query(key, value.clone(), false);

                    if self.request_id == RESERVED_ID {
                        ControlFlow::Continue(BrowserInfoMapVisitorResult::RemoveEntry)
                    } else {
                        // Stop iterating if only canceling a single request.
                        ControlFlow::Break(BrowserInfoMapVisitorResult::RemoveEntry)
                    }
                } else {
                    ControlFlow::Continue(BrowserInfoMapVisitorResult::KeepEntry)
                }
            }
        }

        let visitor = Visitor {
            router,
            context_id,
            request_id,
        };

        browser_query_info_map.find_browser_all(browser_id, &visitor);
    }
}

impl MessageRouterBrowserSide for BrowserSideRouter {
    type Callback = BrowserSideRouterCallback;

    /// Create a new router with the specified configuration.
    fn new(config: MessageRouterConfig) -> Arc<Self> {
        Arc::new_cyclic(|weak_self| {
            let query_message_name = format!("{}{MESSAGE_SUFFIX}", config.js_query_function);
            let cancel_message_name = format!("{}{MESSAGE_SUFFIX}", config.js_cancel_function);
            Self {
                weak_self: weak_self.clone(),
                config,
                query_message_name,
                cancel_message_name,
                query_id_generator: Mutex::new(IdGenerator::new(i64::MAX)),
                handlers: Default::default(),
                browser_query_info_map: Default::default(),
            }
        })
    }

    /// Add a new query handler. If |first| is true it will be added as the first
    /// handler, otherwise it will be added as the last handler. Must be called on
    /// the browser process UI thread.
    fn add_handler(&self, handler: Arc<dyn BrowserSideHandler>, first: bool) -> Option<HandlerId> {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let handler_id = if first {
            let mut handlers = self.handlers.lock().ok()?;
            handlers.offset += 1;
            handlers.handlers.push_front(Some(handler));
            -handlers.offset
        } else {
            let mut handlers = self.handlers.lock().ok()?;
            let offset = i32::try_from(handlers.handlers.len()).ok()? - handlers.offset;
            handlers.handlers.push_back(Some(handler));
            offset
        };

        Some(handler_id)
    }

    /// Remove an existing query handler. Any pending queries associated with the
    /// handler will be canceled. Handler::OnQueryCanceled will be called and the
    /// associated JavaScript onFailure callback will be executed with an error
    /// code of -1. Returns true if the handler is removed successfully or false
    /// if the handler is not found. Must be called on the browser process UI
    /// thread.
    fn remove_handler(&self, handler_id: HandlerId) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let Ok(mut handlers) = self.handlers.lock() else {
            return false;
        };
        let Ok(index) = usize::try_from(handler_id + handlers.offset) else {
            return false;
        };
        let Some(entry) = handlers.handlers.get_mut(index) else {
            return false;
        };
        let handler = entry.take();

        let trim_start = handlers
            .handlers
            .iter()
            .take_while(|entry| entry.is_none())
            .count();
        for _ in 0..trim_start {
            handlers.offset -= 1;
            handlers.handlers.pop_front();
        }

        let trim_end = handlers
            .handlers
            .iter()
            .rev()
            .take_while(|entry| entry.is_none())
            .count();
        for _ in 0..trim_end {
            handlers.handlers.pop_back();
        }

        if let Some(handler) = handler {
            self.cancel_pending_for(None, Some((handler_id, handler)), true);
            true
        } else {
            false
        }
    }

    /// Cancel all pending queries associated with either |browser| or |handler|.
    /// If both |browser| and |handler| are NULL all pending queries will be
    /// canceled. Handler::OnQueryCanceled will be called and the associated
    /// JavaScript onFailure callback will be executed in all cases with an error
    /// code of -1.
    fn cancel_pending(&self, browser: Option<Browser>, handler_id: Option<HandlerId>) {
        let handler = handler_id.and_then(|handler_id| {
            let handlers = self.handlers.lock().ok()?;
            let index = usize::try_from(handler_id + handlers.offset).ok()?;
            let entry = handlers.handlers.get(index)?;
            entry.as_ref().map(|handler| (handler_id, handler.clone()))
        });
        self.cancel_pending_for(browser, handler, true);
    }

    /// Returns the number of queries currently pending for the specified
    /// |browser| and/or |handler|. Either or both values may be empty. Must be
    /// called on the browser process UI thread.
    fn pending_count(&self, browser: Option<Browser>, handler_id: Option<HandlerId>) -> usize {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let Ok(mut browser_query_info_map) = self.browser_query_info_map.lock() else {
            return 0;
        };
        if browser_query_info_map.is_empty() {
            return 0;
        }

        let handler_id = handler_id.and_then(|handler_id| {
            let handlers = self.handlers.lock().ok()?;
            let index = usize::try_from(handler_id + handlers.offset).ok()?;
            let entry = handlers.handlers.get(index)?;
            entry.as_ref().map(|_| handler_id)
        });

        if let Some(handler_id) = handler_id {
            // Need to iterate over each QueryInfo object to test the handler.
            struct Visitor {
                handler_id: HandlerId,
                count: AtomicUsize,
            }

            impl BrowserInfoMapVisitor<i64, BrowserSideQueryInfo> for Visitor {
                fn on_next_info(
                    &self,
                    _browser_id: i32,
                    _key: i64,
                    value: &BrowserSideQueryInfo,
                ) -> ControlFlow<BrowserInfoMapVisitorResult, BrowserInfoMapVisitorResult>
                {
                    if value
                        .handler
                        .as_ref()
                        .map(|handler| handler.0 == self.handler_id)
                        .unwrap_or(false)
                    {
                        self.count.fetch_add(1, Ordering::Relaxed);
                    }

                    ControlFlow::Continue(BrowserInfoMapVisitorResult::KeepEntry)
                }
            }

            let visitor = Visitor {
                handler_id,
                count: Default::default(),
            };

            if let Some(browser) = browser {
                browser_query_info_map.find_browser_all(browser.identifier(), &visitor);
            } else {
                browser_query_info_map.find_all(&visitor);
            }

            visitor.count.load(Ordering::Relaxed)
        } else if let Some(browser) = browser {
            // Count queries associated with the specified browser.
            browser_query_info_map.browser_len(browser.identifier())
        } else {
            // Count all queries for all browsers.
            browser_query_info_map.len()
        }
    }
}

impl MessageRouterBrowserSideHandlerCallbacks for BrowserSideRouter {
    /// Call from CefLifeSpanHandler::OnBeforeClose. Any pending queries
    /// associated with |browser| will be canceled and Handler::OnQueryCanceled
    /// will be called. No JavaScript callbacks will be executed since this
    /// indicates destruction of the browser.
    fn on_before_close(&self, browser: Option<Browser>) {
        self.cancel_pending_for(browser, None, false);
    }

    /// Call from CefRequestHandler::OnRenderProcessTerminated. Any pending
    /// queries associated with |browser| will be canceled and
    /// Handler::OnQueryCanceled will be called. No JavaScript callbacks will be
    /// executed since this indicates destruction of the context.
    fn on_render_process_terminated(&self, browser: Option<Browser>) {
        self.cancel_pending_for(browser, None, false);
    }

    /// Call from CefRequestHandler::OnBeforeBrowse only if the navigation is
    /// allowed to proceed. If |frame| is the main frame then any pending queries
    /// associated with |browser| will be canceled and Handler::OnQueryCanceled
    /// will be called. No JavaScript callbacks will be executed since this
    /// indicates destruction of the context.
    fn on_before_browse(&self, browser: Option<Browser>, frame: Option<Frame>) {
        if frame.map(|frame| frame.is_main() != 0).unwrap_or(false) {
            self.cancel_pending_for(browser, None, false);
        }
    }

    /// Call from CefClient::OnProcessMessageReceived. Returns true if the message
    /// is handled by this router or false otherwise.
    fn on_process_message_received(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        _source_process: ProcessId,
        message: Option<ProcessMessage>,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::UI), 0);
        let Some(message) = message else {
            return true;
        };

        let name = CefString::from(&message.name()).to_string();
        if name == self.query_message_name {
            let content = mru::RenderMessage::from(Some(message));

            let (arc_self, browser_id, query_id, handlers, mut browser_query_info_map) = match (
                self.weak_self.upgrade(),
                browser.as_ref(),
                self.query_id_generator.lock(),
                self.handlers.lock(),
                self.browser_query_info_map.lock(),
            ) {
                (
                    Some(arc_self),
                    Some(browser),
                    Ok(mut query_id_generator),
                    Ok(handlers),
                    Ok(browser_query_info_map),
                ) if !handlers.handlers.is_empty() => (
                    arc_self,
                    browser.identifier(),
                    query_id_generator.next(),
                    handlers.clone(),
                    browser_query_info_map,
                ),
                _ => {
                    self.cancel_unhandled_query(
                        browser,
                        frame,
                        content.context_id,
                        content.request_id,
                    );
                    return true;
                }
            };

            let callback = BrowserSideRouterCallback::new(
                arc_self,
                browser_id,
                query_id,
                content.is_persistent,
                self.config.message_size_threshold,
                name,
            );

            let invoke_handler = {
                Box::new(
                    |handler: &dyn BrowserSideHandler, payload: &mru::MessagePayload| match payload
                    {
                        mru::MessagePayload::Empty => handler.on_query_binary(
                            browser.clone(),
                            frame.clone(),
                            query_id,
                            &mru::EmptyBinaryBuffer,
                            content.is_persistent,
                            callback.clone(),
                        ),
                        mru::MessagePayload::String(payload) => handler.on_query_str(
                            browser.clone(),
                            frame.clone(),
                            query_id,
                            &payload.to_string(),
                            content.is_persistent,
                            callback.clone(),
                        ),
                        mru::MessagePayload::Binary(buffer) => handler.on_query_binary(
                            browser.clone(),
                            frame.clone(),
                            query_id,
                            buffer.as_ref(),
                            content.is_persistent,
                            callback.clone(),
                        ),
                    },
                )
            };
            let handler = handlers
                .handlers
                .iter()
                .enumerate()
                .find_map(move |(index, handler)| {
                    let handler_id = i32::try_from(index).ok()? - handlers.offset;
                    let handler = handler.as_ref()?;
                    if invoke_handler(handler.as_ref(), &content.payload) {
                        Some((handler_id, handler.clone()))
                    } else {
                        None
                    }
                });
            if let Some(handler) = handler {
                // Persist the query information until the callback executes.
                // It's safe to do this here because the callback will execute
                // asynchronously.
                let query_info = BrowserSideQueryInfo {
                    browser,
                    frame,
                    context_id: content.context_id,
                    request_id: content.request_id,
                    is_persistent: content.is_persistent,
                    callback: Some(callback),
                    handler: Some(handler),
                };

                browser_query_info_map.insert(browser_id, query_id, query_info);
            } else {
                if let Ok(mut callback) = callback.lock() {
                    callback.detach();
                }

                // No one chose to handle the query so cancel it.
                self.cancel_unhandled_query(browser, frame, content.context_id, content.request_id);
            }

            return true;
        } else if name == self.cancel_message_name {
            let (browser, args) = match (browser, message.argument_list()) {
                (Some(browser), Some(args)) => (browser, args),
                _ => return true,
            };
            debug_assert_eq!(args.size(), 2);

            let browser_id = browser.identifier();
            let context_id = args.int(0);
            let request_id = args.int(1);

            self.cancel_pending_request(browser_id, context_id, request_id);
            return true;
        }

        false
    }
}

wrap_task! {
    struct BrowserSideRouterCancelPendingFor {
        router: Weak<BrowserSideRouter>,
        browser: Option<Browser>,
        handler: Option<(HandlerId, Arc<dyn BrowserSideHandler>)>,
        notify_renderer: bool,
    }

    impl Task {
       fn execute(&self) {
           let Some(router) = self.router.upgrade() else {
               return;
           };

           router.cancel_pending_for(self.browser.clone(), self.handler.clone(), self.notify_renderer);
       }
    }
}

/// Structure representing a pending request.
#[derive(Clone, Default)]
pub struct RendererSideRequestInfo {
    /// True if the request is persistent.
    is_persistent: bool,
    /// Success callback function. May be [`None`].
    success_callback: Option<V8Value>,
    /// Failure callback function. May be [`None`].
    failure_callback: Option<V8Value>,
}

pub struct RendererSideRouter {
    weak_self: Weak<Self>,
    config: MessageRouterConfig,
    query_message_name: String,
    cancel_message_name: String,
    context_id_generator: Mutex<IdGenerator<i32>>,
    request_id_generator: Mutex<IdGenerator<i32>>,
    browser_request_info_map: Mutex<BrowserInfoMap<(i32, i32), RendererSideRequestInfo>>,
    context_map: Mutex<BTreeMap<i32, Option<V8Context>>>,
}

impl RendererSideRouter {
    /// Retrieve a RequestInfo object from the map based on the renderer-side
    /// IDs. If |always_remove| is true then the RequestInfo object will always be
    /// removed from the map. Othewise, the RequestInfo object will only be removed
    /// if the query is non-persistent.
    fn get_request_info(
        &self,
        browser_id: i32,
        context_id: i32,
        request_id: i32,
        always_remove: bool,
    ) -> Option<RendererSideRequestInfo> {
        let Ok(mut browser_request_info_map) = self.browser_request_info_map.lock() else {
            return None;
        };

        struct Visitor {
            always_remove: bool,
        }

        impl BrowserInfoMapVisitor<(i32, i32), RendererSideRequestInfo> for Visitor {
            fn on_next_info(
                &self,
                _browser_id: i32,
                _key: (i32, i32),
                value: &RendererSideRequestInfo,
            ) -> ControlFlow<BrowserInfoMapVisitorResult, BrowserInfoMapVisitorResult> {
                ControlFlow::Continue(if self.always_remove || !value.is_persistent {
                    BrowserInfoMapVisitorResult::RemoveEntry
                } else {
                    BrowserInfoMapVisitorResult::KeepEntry
                })
            }
        }

        let visitor = Visitor { always_remove };
        browser_request_info_map.find(browser_id, (context_id, request_id), Some(&visitor))
    }

    /// Returns the new request ID.
    fn send_query(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        context_id: i32,
        request: V8Value,
        request_info: RendererSideRequestInfo,
    ) -> i32 {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let (
            Some(browser),
            Some(frame),
            Ok(mut request_id_generator),
            Ok(mut browser_request_info_map),
        ) = (
            browser,
            frame,
            self.request_id_generator.lock(),
            self.browser_request_info_map.lock(),
        )
        else {
            return RESERVED_ID;
        };
        let request_id = request_id_generator.next();
        let persistent = request_info.is_persistent;
        browser_request_info_map.insert(
            browser.identifier(),
            (context_id, request_id),
            request_info,
        );

        let mut message = mru::build_renderer_message(
            self.config.message_size_threshold,
            &self.query_message_name,
            context_id,
            request_id,
            Some(&request),
            persistent,
        );
        frame.send_process_message(ProcessId::BROWSER, message.as_mut());

        request_id
    }

    /// If |request_id| is kReservedId all requests associated with |context_id|
    /// will be canceled, otherwise only the specified |request_id| will be
    /// canceled. Returns true if any request was canceled.
    fn send_cancel(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        context_id: i32,
        request_id: i32,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let (Some(browser_id), Some(frame)) = (browser.as_ref().map(Browser::identifier), frame)
        else {
            return false;
        };

        let cancel_count = if request_id != RESERVED_ID {
            // Cancel a single request.
            if self
                .get_request_info(browser_id, context_id, request_id, true)
                .is_some()
            {
                1
            } else {
                0
            }
        } else {
            // Cancel all requests with the specified context ID.
            struct Visitor {
                context_id: i32,
                cancel_count: AtomicUsize,
            }

            impl BrowserInfoMapVisitor<(i32, i32), RendererSideRequestInfo> for Visitor {
                fn on_next_info(
                    &self,
                    _browser_id: i32,
                    key: (i32, i32),
                    _value: &RendererSideRequestInfo,
                ) -> ControlFlow<BrowserInfoMapVisitorResult, BrowserInfoMapVisitorResult>
                {
                    ControlFlow::Continue(if self.context_id == key.0 {
                        self.cancel_count.fetch_add(1, Ordering::Relaxed);
                        BrowserInfoMapVisitorResult::RemoveEntry
                    } else {
                        BrowserInfoMapVisitorResult::KeepEntry
                    })
                }
            }

            let visitor = Visitor {
                context_id,
                cancel_count: Default::default(),
            };

            let Ok(mut browser_request_info_map) = self.browser_request_info_map.lock() else {
                return false;
            };
            browser_request_info_map.find_browser_all(browser_id, &visitor);
            visitor.cancel_count.load(Ordering::Relaxed)
        };

        if cancel_count > 0 {
            let mut message =
                process_message_create(Some(&CefString::from(self.cancel_message_name.as_str())));
            if let Some(args) = message.as_ref().and_then(ProcessMessage::argument_list) {
                args.set_int(0, context_id);
                args.set_int(1, request_id);

                frame.send_process_message(ProcessId::BROWSER, message.as_mut());
            }

            true
        } else {
            false
        }
    }

    fn execute_success_callback(
        &self,
        browser_id: i32,
        context_id: i32,
        request_id: i32,
        response: mru::MessagePayload,
    ) {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let Some(success_callback) = self
            .get_request_info(browser_id, context_id, request_id, false)
            .and_then(|info| info.success_callback)
        else {
            return;
        };

        let Some(mut context) = self.get_context_by_id(context_id) else {
            return;
        };

        let value = match &response {
            mru::MessagePayload::String(s) => v8_value_create_string(Some(&s.into())),
            mru::MessagePayload::Empty | mru::MessagePayload::Binary(_) => {
                let data = match &response {
                    mru::MessagePayload::Binary(b) => b.data(),
                    _ => &[],
                };

                if context.enter() == 0 {
                    return;
                }

                #[cfg(feature = "sandbox")]
                let value =
                    v8_value_create_array_buffer_with_copy(data.as_ptr() as *mut u8, data.len());
                #[cfg(not(feature = "sandbox"))]
                let value = v8_value_create_array_buffer(
                    data.as_ptr() as *mut u8,
                    data.len(),
                    Some(&mut mru::BinaryValueArrayBufferReleaseCallback::new(
                        response,
                    )),
                );

                context.exit();
                value
            }
        };

        success_callback.execute_function_with_context(Some(&mut context), None, Some(&[value]));
    }

    fn execute_failure_callback(
        &self,
        browser_id: i32,
        context_id: i32,
        request_id: i32,
        error_code: i32,
        error_message: &str,
    ) {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let Some(failure_callback) = self
            .get_request_info(browser_id, context_id, request_id, true)
            .and_then(|info| info.failure_callback)
        else {
            return;
        };

        let Some(mut context) = self.get_context_by_id(context_id) else {
            return;
        };

        failure_callback.execute_function_with_context(
            Some(&mut context),
            None,
            Some(&[
                v8_value_create_int(error_code),
                v8_value_create_string(Some(&CefString::from(error_message))),
            ]),
        );
    }

    fn create_id_for_context(&self, context: V8Context) -> i32 {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);

        // The context should not already have an associated ID.
        debug_assert_eq!(self.get_id_for_context(context.clone(), false), RESERVED_ID);

        let (Ok(mut context_id_generator), Ok(mut context_map)) =
            (self.context_id_generator.lock(), self.context_map.lock())
        else {
            return RESERVED_ID;
        };
        let context_id = context_id_generator.next();
        context_map.insert(context_id, Some(context));
        context_id
    }

    fn get_id_for_context(&self, mut context: V8Context, remove: bool) -> i32 {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let Ok(mut context_map) = self.context_map.lock() else {
            return RESERVED_ID;
        };
        let context_id = context_map
            .iter()
            .find(|(_, entry)| {
                entry
                    .as_ref()
                    .map_or(0, |entry| entry.is_same(Some(&mut context)))
                    != 0
            })
            .map(|(context_id, _)| *context_id);

        if let Some(context_id) = context_id {
            if remove {
                context_map.remove(&context_id);
            }
            context_id
        } else {
            RESERVED_ID
        }
    }

    fn get_context_by_id(&self, context_id: i32) -> Option<V8Context> {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let context_map = self.context_map.lock().ok()?;
        context_map.get(&context_id)?.clone()
    }
}

impl MessageRouterRendererSide for RendererSideRouter {
    /// Create a new router with the specified configuration.
    fn new(config: MessageRouterConfig) -> Arc<Self> {
        let query_message_name = format!("{}{MESSAGE_SUFFIX}", config.js_query_function);
        let cancel_message_name = format!("{}{MESSAGE_SUFFIX}", config.js_cancel_function);
        Arc::new_cyclic(|weak_self| Self {
            weak_self: weak_self.clone(),
            config,
            query_message_name,
            cancel_message_name,
            context_id_generator: Mutex::new(IdGenerator::new(i32::MAX)),
            request_id_generator: Mutex::new(IdGenerator::new(i32::MAX)),
            browser_request_info_map: Default::default(),
            context_map: Default::default(),
        })
    }

    /// Returns the number of queries currently pending for the specified
    /// |browser| and/or |context|. Either or both values may be empty.
    fn pending_count(&self, browser: Option<Browser>, context: Option<V8Context>) -> usize {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let Ok(mut browser_request_info_map) = self.browser_request_info_map.lock() else {
            return 0;
        };
        if browser_request_info_map.is_empty() {
            return 0;
        }

        if let Some(context) = context {
            let context_id = self.get_id_for_context(context, false);
            if context_id == RESERVED_ID {
                // Nothing associated with the specified context.
                return 0;
            }

            // Need to iterate over each RequestInfo object to test the context.
            struct Visitor {
                context_id: i32,
                count: AtomicUsize,
            }

            impl BrowserInfoMapVisitor<(i32, i32), RendererSideRequestInfo> for Visitor {
                fn on_next_info(
                    &self,
                    _browser_id: i32,
                    key: (i32, i32),
                    _value: &RendererSideRequestInfo,
                ) -> ControlFlow<BrowserInfoMapVisitorResult, BrowserInfoMapVisitorResult>
                {
                    if key.0 == self.context_id {
                        self.count.fetch_add(1, Ordering::Relaxed);
                    }
                    ControlFlow::Continue(BrowserInfoMapVisitorResult::KeepEntry)
                }
            }

            let visitor = Visitor {
                context_id,
                count: Default::default(),
            };

            if let Some(browser) = browser {
                browser_request_info_map.find_browser_all(browser.identifier(), &visitor);
            } else {
                browser_request_info_map.find_all(&visitor);
            }

            visitor.count.load(Ordering::Relaxed)
        } else if let Some(browser) = browser {
            browser_request_info_map.browser_len(browser.identifier())
        } else {
            browser_request_info_map.len()
        }
    }
}

impl MessageRouterRendererSideHandlerCallbacks for RendererSideRouter {
    /// Call from CefRenderProcessHandler::OnContextCreated. Registers the
    /// JavaScripts functions with the new context.
    fn on_context_created(
        &self,
        _browser: Option<Browser>,
        _frame: Option<Frame>,
        context: Option<V8Context>,
    ) {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);

        // Register function handlers with the 'window' object.
        let Some(window) = context.and_then(|context| context.global()) else {
            return;
        };

        let mut handler = RendererSideV8Handler::new(
            self.weak_self.clone(),
            self.config.clone(),
            Default::default(),
        );
        let attributes = sys::cef_v8_propertyattribute_t(
            [
                sys::cef_v8_propertyattribute_t::V8_PROPERTY_ATTRIBUTE_READONLY,
                sys::cef_v8_propertyattribute_t::V8_PROPERTY_ATTRIBUTE_DONTENUM,
                sys::cef_v8_propertyattribute_t::V8_PROPERTY_ATTRIBUTE_DONTDELETE,
            ]
            .into_iter()
            .fold(0, |acc, attr| acc | attr.0),
        )
        .into();

        // Add the query function.
        let name = CefString::from(self.config.js_query_function.as_str());
        let mut query_func = v8_value_create_function(Some(&name), Some(&mut handler));
        window.set_value_bykey(Some(&name), query_func.as_mut(), attributes);

        // Add the cancel function.
        let name = CefString::from(self.config.js_cancel_function.as_str());
        let mut cancel_func = v8_value_create_function(Some(&name), Some(&mut handler));
        window.set_value_bykey(Some(&name), cancel_func.as_mut(), attributes);
    }

    /// Call from CefRenderProcessHandler::OnContextReleased. Any pending queries
    /// associated with the released context will be canceled and
    /// Handler::OnQueryCanceled will be called in the browser process.
    fn on_context_released(
        &self,
        browser: Option<Browser>,
        frame: Option<Frame>,
        context: Option<V8Context>,
    ) {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);

        // Get the context ID and remove the context from the map.
        let context_id = context
            .map(|context| self.get_id_for_context(context, true))
            .unwrap_or(RESERVED_ID);
        if context_id != RESERVED_ID {
            // Cancel all pending requests for the context.
            self.send_cancel(browser, frame, context_id, RESERVED_ID);
        }
    }

    /// Call from CefRenderProcessHandler::OnProcessMessageReceived. Returns true
    /// if the message is handled by this router or false otherwise.
    fn on_process_message_received(
        &self,
        browser: Option<Browser>,
        _frame: Option<Frame>,
        _source_process: Option<ProcessId>,
        message: Option<ProcessMessage>,
    ) -> bool {
        debug_assert_ne!(currently_on(ThreadId::RENDERER), 0);
        let (Some(browser), Some(message)) = (browser, message) else {
            return true;
        };

        let name = CefString::from(&message.name()).to_string();
        if name != self.query_message_name {
            return false;
        }

        let content = mru::BrowserMessage::from(Some(message));
        if content.is_success {
            self.execute_success_callback(
                browser.identifier(),
                content.context_id,
                content.request_id,
                content.payload,
            );
        } else {
            let error_message = match content.payload {
                mru::MessagePayload::String(s) => s.to_string(),
                _ => Default::default(),
            };
            self.execute_failure_callback(
                browser.identifier(),
                content.context_id,
                content.request_id,
                content.error_code,
                error_message.as_str(),
            );
        }

        true
    }
}

wrap_v8_handler! {
    struct RendererSideV8Handler {
        router: Weak<RendererSideRouter>,
        config: MessageRouterConfig,
        context_id: OnceLock<i32>,
    }

    impl V8Handler {
        fn execute(
            &self,
            name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            retval: Option<&mut Option<V8Value>>,
            exception: Option<&mut CefString>,
        ) -> i32 {
            macro_rules! return_exception {
                ($message:expr) => {
                    if let Some(exception) = exception {
                        *exception = CefString::from($message);
                    }

                    return 1;
                };
            }

            let Some(name) = name else {
                return_exception!("Missing function name");
            };
            let name = name.to_string();
            if name == self.config.js_query_function {
                let Some(arg) = arguments
                    .filter(|arguments| arguments.len() == 1)
                    .and_then(|arguments| arguments[0].as_ref())
                    .filter(|arg| arg.is_object() != 0)
                else {
                    return_exception!("Invalid arguments; expecting a single object");
                };

                let key = CefString::from(ObjectMember::REQUEST);
                let Some(request) = arg.value_bykey(Some(&key)) else {
                    return_exception!(format!(
                        "Invalid arguments; object member '{}' is required",
                        ObjectMember::REQUEST
                    )
                    .as_str());
                };
                if request.is_string() == 0 && request.is_array_buffer() == 0 {
                    return_exception!(format!("Invalid arguments; object member '{}' must have type string or ArrayBuffer", ObjectMember::REQUEST).as_str());
                }

                let key = CefString::from(ObjectMember::ON_SUCCESS);
                let success = if let Some(success) = arg.value_bykey(Some(&key)) {
                    if success.is_function() == 0 {
                        return_exception!(format!(
                            "Invalid arguments; object member '{}' must have type function",
                            ObjectMember::ON_SUCCESS
                        )
                        .as_str());
                    }
                    Some(success)
                } else {
                    None
                };

                let key = CefString::from(ObjectMember::ON_FAILURE);
                let failure = if let Some(failure) = arg.value_bykey(Some(&key)) {
                    if failure.is_function() == 0 {
                        return_exception!(format!(
                            "Invalid arguments; object member '{}' must have type function",
                            ObjectMember::ON_FAILURE
                        )
                        .as_str());
                    }
                    Some(failure)
                } else {
                    None
                };

                let key = CefString::from(ObjectMember::PERSISTENT);
                let persistent = if let Some(persistent) = arg.value_bykey(Some(&key)).filter(|v| v.is_undefined() == 0) {
                    if persistent.is_bool() == 0 {
                        return_exception!(format!(
                            "Invalid arguments; object member '{}' must have type boolean",
                            ObjectMember::PERSISTENT
                        )
                        .as_str());
                    }
                    Some(persistent)
                } else {
                    None
                };

                if let (Some(router), Some(context)) =
                    (self.router.upgrade(), v8_context_get_current_context())
                {
                    let context_id = self.get_id_for_context(context.clone());
                    let persistent = persistent.map_or(0, |value| value.bool_value()) != 0;
                    let request_id = router.send_query(
                        context.browser(),
                        context.frame(),
                        context_id,
                        request,
                        RendererSideRequestInfo {
                            is_persistent: persistent,
                            success_callback: success,
                            failure_callback: failure,
                        },
                    );

                    if let Some(retval) = retval {
                        *retval = v8_value_create_int(request_id);
                    }
                    return 1;
                }
            } else if name == self.config.js_cancel_function {
                let Some(arg) = arguments
                    .filter(|arguments| arguments.len() == 1)
                    .and_then(|arguments| arguments[0].as_ref())
                    .filter(|arg| arg.is_int() != 0)
                else {
                    return_exception!("Invalid arguments; expecting a single integer");
                };

                let request_id = arg.int_value();
                if request_id != RESERVED_ID {
                    if let (Some(router), Some(context)) =
                        (self.router.upgrade(), v8_context_get_current_context())
                    {
                        let context_id = self.get_id_for_context(context.clone());
                        let result = router.send_cancel(
                            context.browser(),
                            context.frame(),
                            context_id,
                            request_id,
                        );

                        if let Some(retval) = retval {
                            *retval = v8_value_create_bool(result.into());
                        }
                        return 1;
                    }
                }
            }

            0
        }
    }
}

impl RendererSideV8Handler {
    fn get_id_for_context(&self, context: V8Context) -> i32 {
        *self.context_id.get_or_init(|| {
            let Some(router) = self.router.upgrade() else {
                return RESERVED_ID;
            };
            router.create_id_for_context(context)
        })
    }
}
