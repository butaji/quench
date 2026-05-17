// Module: main.r

use protocol::{AppState, Filter, Task};

#[derive(Debug, Clone)]
pub struct Person {
    pub id: f64,
    pub name: String,
    pub age: f64,
    pub salary: f64,
}

#[derive(Debug, Clone)]
pub struct Stats {
    pub count: f64,
    pub sum: f64,
    pub average: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone)]
pub struct Filter {
    pub min_age: f64,
    pub max_age: f64,
    pub min_salary: f64,
}

pub fn first(arr: Vec<T>) -> Option<T> {
    if (arr.len() > 0i32) {
                return arr.get(0i32);
    }
        return None;
}

pub fn last(arr: Vec<T>) -> Option<T> {
    if (arr.len() > 0i32) {
                return arr.get(arr.len() - 1i32);
    }
        return None;
}

pub fn map(arr: Vec<T>, transform: ()) -> Vec<U> {
    let result: Vec<U> = vec![];
    for i: i32 = 0i32; (i < arr.len()); i += 1 {
        {
            result.push(transform(arr.get(i)));
        }
    }
        return result;
}

pub fn filter(arr: Vec<T>, predicate: ()) -> Vec<T> {
    let result: Vec<T> = vec![];
    for i: i32 = 0i32; (i < arr.len()); i += 1 {
        {
            if predicate(arr.get(i)) {
                result.push(arr.get(i));
            }
        }
    }
        return result;
}

pub fn reduce(arr: Vec<T>, reducer: (), initial: f64) -> f64 {
    let acc: () = initial;
    for i: i32 = 0i32; (i < arr.len()); i += 1 {
        {
            acc = reducer(acc, arr.get(i));
        }
    }
        return acc;
}

pub fn filter_people(people: Vec<Person>, criteria: Filter) -> Vec<Person> {
        return filter(people, |p| {     if criteria.minAge != undefined && (p.age < criteria.minAge) {
                return false;
    }
    if criteria.maxAge != undefined && (p.age > criteria.maxAge) {
                return false;
    }
    if criteria.minSalary != undefined && (p.salary < criteria.minSalary) {
                return false;
    }
        return true;
 });
}

pub fn calculate_stats(values: Vec<f64>) -> Stats {
    if values.len() == 0i32 {
                return Stats { count: 0i32, sum: 0i32, average: 0i32, min: 0i32, max: 0i32 };
    }
    let sum: i32 = 0i32;
    let min: () = values.get(0i32);
    let max: () = values.get(0i32);
    for i: i32 = 0i32; (i < values.len()); i += 1 {
        {
            sum = sum + values.get(i);
            if (values.get(i) < min) {
                min = values.get(i);
            }
            if (values.get(i) > max) {
                max = values.get(i);
            }
        }
    }
        return Stats { count: values.len(), sum: sum, average: sum / values.len(), min: min, max: max };
}

pub fn group_by(arr: Vec<T>, key: ()) -> Record<String> {
    let result: Record = Record {  };
    for i: i32 = 0i32; (i < arr.len()); i += 1 {
        {
            let item: () = arr.get(i);
            let group_key: () = key(item);
            if !result.get(group_key) {
                result[group_key] = vec![];
            }
            result.get(group_key).push(item);
        }
    }
        return result;
}

pub fn sort_by(arr: Vec<T>, key: ()) -> Vec<T> {
    let sorted: () = arr.as_slice()[];
    for i: i32 = 0i32; (i < sorted.len()); i += 1 {
        {
            for j: i32 = 0i32; (j < sorted.len() - i - 1i32); j += 1 {
                {
                    if (key(sorted.get(j)) > key(sorted.get(j + 1i32))) {
                        let temp: () = sorted.get(j);
                        sorted[j] = sorted.get(j + 1i32);
                        sorted[j + 1i32] = temp;
                    }
                }
            }
        }
    }
        return sorted;
}


