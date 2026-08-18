use std::cell::RefCell;

thread_local! {
    static FAMILIES: RefCell<Option<Vec<&'static str>>> = const { RefCell::new(None) };
}

pub fn record(family: &'static str) {
    FAMILIES.with(|slot| {
        if let Some(families) = slot.borrow_mut().as_mut() {
            families.push(family);
        }
    });
}

#[cfg(test)]
pub async fn capture_async<F, Fut, T>(operation: F) -> (T, Vec<&'static str>)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    FAMILIES.with(|slot| {
        *slot.borrow_mut() = Some(Vec::new());
    });
    let value = operation().await;
    let families = FAMILIES.with(|slot| slot.borrow_mut().take().unwrap_or_default());
    (value, families)
}
