//! Deferred per-browser view mutations (editable hint, URL row), drained on Bevy `Update`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::{Query, Res, Resource};

use crate::browser::cef::entity::BrowserId;
use crate::browser::cef::facet::{EditableFocusHint, LastAddressUrl};

#[derive(Debug, Clone)]
pub enum BrowserUiOp {
    InvalidateEditableFocusHint { browser_id: i32 },
    SetEditableFocusHint { browser_id: i32, editable: bool },
    RemoveBrowserEntries { browser_id: i32 },
}

#[derive(Resource, Clone)]
pub struct BrowserUiOpQueue(pub Arc<Mutex<VecDeque<BrowserUiOp>>>);

impl Default for BrowserUiOpQueue {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(VecDeque::new())))
    }
}

pub fn enqueue_browser_ui_op(queue: &BrowserUiOpQueue, op: BrowserUiOp) {
    if let Ok(mut q) = queue.0.lock() {
        q.push_back(op);
    }
}

pub fn apply_browser_ui_ops_system(
    queue: Res<BrowserUiOpQueue>,
    mut hints: Query<(&BrowserId, &mut EditableFocusHint, &mut LastAddressUrl)>,
) {
    let ops: Vec<BrowserUiOp> = queue
        .0
        .lock()
        .ok()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default();
    if ops.is_empty() {
        return;
    }
    let mut missed: Vec<BrowserUiOp> = Vec::new();
    for op in ops {
        match op {
            BrowserUiOp::InvalidateEditableFocusHint { browser_id } => {
                let mut hit = false;
                for (bid, mut hint, _) in &mut hints {
                    if bid.0 == browser_id {
                        hint.0 = None;
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    missed.push(BrowserUiOp::InvalidateEditableFocusHint { browser_id });
                }
            }
            BrowserUiOp::SetEditableFocusHint {
                browser_id,
                editable,
            } => {
                let mut hit = false;
                for (bid, mut hint, _) in &mut hints {
                    if bid.0 == browser_id {
                        hint.0 = Some(editable);
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    missed.push(BrowserUiOp::SetEditableFocusHint {
                        browser_id,
                        editable,
                    });
                }
            }
            BrowserUiOp::RemoveBrowserEntries { browser_id } => {
                let mut hit = false;
                for (bid, mut hint, mut last_url) in &mut hints {
                    if bid.0 == browser_id {
                        hint.0 = None;
                        last_url.0 = None;
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    missed.push(BrowserUiOp::RemoveBrowserEntries { browser_id });
                }
            }
        }
    }
    if missed.is_empty() {
        return;
    }
    if let Ok(mut q) = queue.0.lock() {
        const CAP: usize = 64;
        for op in missed.into_iter().take(CAP) {
            q.push_back(op);
        }
    }
}
