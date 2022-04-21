pub struct Tun<E, S> {
    stream: S,
    executor: E,
}
