struct Channel {
    name: String,
    id: i32,
    handlers: Vec<Box<dyn Fn(&str) + Send>>,
}
