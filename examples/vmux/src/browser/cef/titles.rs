use std::sync::Mutex;

static PENDING: Mutex<Vec<(i32, String)>> = Mutex::new(Vec::new());

pub fn push_title(browser_id: i32, title: String) {
    if let Ok(mut g) = PENDING.lock() {
        g.push((browser_id, title));
    }
}

pub fn drain_titles() -> Vec<(i32, String)> {
    PENDING
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
}
