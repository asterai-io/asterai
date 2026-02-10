use crate::bindings::exports::___USERNAME_SNAKE___::___COMPONENT_SNAKE___::___COMPONENT_SNAKE___::Guest;

#[allow(warnings)]
mod bindings {
    wit_bindgen::generate!({
        path: "wit/package.wasm",
        world: "component",
    });
}

struct Component;

impl Guest for Component {
    fn greet(name: String) {
        let greeting = format!("hello {name}!");
        println!("{greeting}");
    }
}

bindings::export!(Component with_types_in bindings);
