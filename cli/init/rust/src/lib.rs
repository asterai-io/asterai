use crate::bindings::asterai::host::api;
use crate::bindings::exports::your_username::greeter::greeter::Guest;

#[allow(warnings)]
mod bindings;

struct Component;

impl Guest for Component {
    fn greet(name: String) {
        let greeting = format!("hello {name}");
        api::send_response_to_agent(&greeting);
    }
}

bindings::export!(Component with_types_in bindings);
