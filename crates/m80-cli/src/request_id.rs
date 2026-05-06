//! Scoped CLI request id for one `m80 run` invocation.

use std::cell::RefCell;

thread_local! {
    static CURRENT: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) struct Scope {
    previous: Option<String>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|slot| {
            *slot.borrow_mut() = previous;
        });
    }
}

pub(crate) fn new() -> String {
    format!("req_{}", ulid::Ulid::new())
}

pub(crate) fn set(request_id: String) -> Scope {
    let previous = CURRENT.with(|slot| slot.replace(Some(request_id)));
    Scope { previous }
}

pub(crate) fn current() -> Option<String> {
    CURRENT.with(|slot| slot.borrow().clone())
}
