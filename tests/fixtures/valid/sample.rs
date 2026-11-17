use std::collections::HashMap;

pub struct Value {
    number: i32,
}

fn add_numbers(left: i32, right: i32) -> i32 {
    let mut values = HashMap::new();
    values.insert("sum", left + right);
    values["sum"]
}
