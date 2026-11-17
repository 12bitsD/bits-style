use std::collections::HashMap;

static mut COUNTER: i32 = 0;

fn computeResult(first: i32, second: i32, third: i32, fourth: i32, fifth: bool) -> i32 {
    let result = first + second + third + fourth + fifth;
    if result > 0 {
        return result;
    }
    result
}
