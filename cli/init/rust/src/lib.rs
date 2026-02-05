use crate::bindings::asterai::host::api;
use crate::bindings::exports::___USERNAME_SNAKE___::___COMPONENT___::___COMPONENT___::Guest;

#[allow(warnings)]
mod bindings;

struct Component;

impl Guest for Component {
    fn greet(name: String) {
        let greeting = format!("hello {name}");
        println!("{greeting}");
    }
}

bindings::export!(Component with_types_in bindings);
