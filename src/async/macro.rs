#[macro_export]
macro_rules! ready {
    ($poll: expr) => {
        match $poll {
            std::task::Poll::Pending => return std::task::Poll::Pending,
            std::task::Poll::Ready(ready) => ready,
        }
    };
}
