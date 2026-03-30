#![allow(unsafe_code)] // Explicit runtime bridge from symbolic foreign bindings to raw host pointers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    Int32,
    Float32,
    Float64,
    Bool,
    Void,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Int32(i32),
    Float32(f32),
    Float64(f64),
    Bool(bool),
    Void,
}

#[must_use]
pub fn invoke(addr: usize, ret: ScalarType, args: &[Value]) -> Option<Value> {
    match (ret, args) {
        (ScalarType::Void, []) => {
            let f: extern "C" fn() = unsafe { std::mem::transmute(addr) };
            f();
            Some(Value::Void)
        }
        (ScalarType::Int32, []) => {
            let f: extern "C" fn() -> i32 = unsafe { std::mem::transmute(addr) };
            Some(Value::Int32(f()))
        }
        (ScalarType::Float32, []) => {
            let f: extern "C" fn() -> f32 = unsafe { std::mem::transmute(addr) };
            Some(Value::Float32(f()))
        }
        (ScalarType::Float64, []) => {
            let f: extern "C" fn() -> f64 = unsafe { std::mem::transmute(addr) };
            Some(Value::Float64(f()))
        }
        (ScalarType::Bool, []) => {
            let f: extern "C" fn() -> bool = unsafe { std::mem::transmute(addr) };
            Some(Value::Bool(f()))
        }

        (ScalarType::Void, [Value::Float32(a0)]) => {
            let f: extern "C" fn(f32) = unsafe { std::mem::transmute(addr) };
            f(*a0);
            Some(Value::Void)
        }
        (ScalarType::Float32, [Value::Float32(a0)]) => {
            let f: extern "C" fn(f32) -> f32 = unsafe { std::mem::transmute(addr) };
            Some(Value::Float32(f(*a0)))
        }
        (ScalarType::Float64, [Value::Float64(a0)]) => {
            let f: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(addr) };
            Some(Value::Float64(f(*a0)))
        }
        (ScalarType::Int32, [Value::Int32(a0)]) => {
            let f: extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(addr) };
            Some(Value::Int32(f(*a0)))
        }
        (ScalarType::Bool, [Value::Bool(a0)]) => {
            let f: extern "C" fn(bool) -> bool = unsafe { std::mem::transmute(addr) };
            Some(Value::Bool(f(*a0)))
        }

        (ScalarType::Void, [Value::Float32(a0), Value::Float32(a1)]) => {
            let f: extern "C" fn(f32, f32) = unsafe { std::mem::transmute(addr) };
            f(*a0, *a1);
            Some(Value::Void)
        }
        (ScalarType::Float32, [Value::Float32(a0), Value::Float32(a1)]) => {
            let f: extern "C" fn(f32, f32) -> f32 = unsafe { std::mem::transmute(addr) };
            Some(Value::Float32(f(*a0, *a1)))
        }
        (ScalarType::Void, [Value::Float64(a0), Value::Float64(a1)]) => {
            let f: extern "C" fn(f64, f64) = unsafe { std::mem::transmute(addr) };
            f(*a0, *a1);
            Some(Value::Void)
        }
        (ScalarType::Float64, [Value::Float64(a0), Value::Float64(a1)]) => {
            let f: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(addr) };
            Some(Value::Float64(f(*a0, *a1)))
        }
        (ScalarType::Void, [Value::Int32(a0), Value::Int32(a1)]) => {
            let f: extern "C" fn(i32, i32) = unsafe { std::mem::transmute(addr) };
            f(*a0, *a1);
            Some(Value::Void)
        }
        (ScalarType::Int32, [Value::Int32(a0), Value::Int32(a1)]) => {
            let f: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(addr) };
            Some(Value::Int32(f(*a0, *a1)))
        }
        (ScalarType::Void, [Value::Bool(a0), Value::Bool(a1)]) => {
            let f: extern "C" fn(bool, bool) = unsafe { std::mem::transmute(addr) };
            f(*a0, *a1);
            Some(Value::Void)
        }
        (ScalarType::Bool, [Value::Bool(a0), Value::Bool(a1)]) => {
            let f: extern "C" fn(bool, bool) -> bool = unsafe { std::mem::transmute(addr) };
            Some(Value::Bool(f(*a0, *a1)))
        }

        _ => None,
    }
}
