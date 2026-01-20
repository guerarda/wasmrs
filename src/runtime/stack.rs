use crate::runtime::{store::FuncAddr, value::Value};

#[derive(Debug)]
pub enum StackEntry {
    Value(Value),
    Label,
    Activation(Frame),
}

#[derive(Debug)]
pub struct Frame {
    pub arity: u32,
    pub funcaddr: FuncAddr,
    pub locals: Vec<Value>,
    pub pc: isize,
    pub sp: usize,
}
