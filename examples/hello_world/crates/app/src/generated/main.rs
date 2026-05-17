// Module: main.r

use protocol::{AppState, Filter, Task};

#[derive(Debug, Clone)]
pub struct Greeting {
    pub message: String,
    pub count: f64,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    pub value: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct Stats {
    pub sum: f64,
    pub average: f64,
    pub count: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Pending,
    Active { duration: f64 },
    Completed { timestamp: f64 },
}

pub fn create_greeting(name: String) -> Greeting {
        return Greeting { message: format!("Hello, {}!", name), count: name.len() };
}

pub fn format_names(names: Vec<String>) -> String {
    if names.len() == 0i32 {
                return "No names provided";
    }
    if names.len() == 1i32 {
                return names.get(0i32);
    }
    if names.len() == 2i32 {
                return format!("{} and {}", (), ());
    }
    let last: () = names.get(names.len() - 1i32);
    let rest: () = names.as_slice()[0i32 as usize..-1i32 as usize];
        return format!("{}, and {}", (), last);
}

pub fn calculate_stats(numbers: Vec<f64>) -> Stats {
    if numbers.len() == 0i32 {
                return Stats { sum: 0i32, average: 0i32, count: 0i32 };
    }
    let sum: i32 = 0i32;
    for i: i32 = 0i32; (i < numbers.len()); i += 1 {
        {
            sum = sum + numbers.get(i);
        }
    }
        return Stats { sum: sum, average: sum / numbers.len(), count: numbers.len() };
}

pub fn filter_by_length(names: Vec<String>, min_length: f64) -> Vec<String> {
    let result: Vec<String> = vec![];
    for i: i32 = 0i32; (i < names.len()); i += 1 {
        {
            if (names.get(i).len() >= min_length) {
                result.push(names.get(i));
            }
        }
    }
        return result;
}

pub fn validate_username(username: String) -> ValidationResult {
    if username.len() == 0i32 {
                return ValidationResult { ok: false, error: "Username cannot be empty" };
    }
    if (username.len() < 3i32) {
                return ValidationResult { ok: false, error: "Username must be at least 3 characters" };
    }
    if (username.len() > 20i32) {
                return ValidationResult { ok: false, error: "Username must be at most 20 characters" };
    }
        return ValidationResult { ok: true, value: username };
}

pub fn find_first_long_name(names: Vec<String>, min_length: f64) -> Option<String> {
    for i: i32 = 0i32; (i < names.len()); i += 1 {
        {
            if (names.get(i).len() >= min_length) {
                                return names.get(i);
            }
        }
    }
        return None;
}

pub fn get_status_message(status: Status) -> String {
    match status.tag {
        "pending" =>  {
                        return "Operation is pending...";
        }
        "active" =>  {
                        return format!("Active for {} seconds", ());
        }
        "completed" =>  {
                        return "Operation completed";
        }
    }
}


