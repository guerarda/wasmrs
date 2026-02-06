use crate::runtime::{store::FuncAddr, value::Value};

#[derive(Debug, Clone)]
pub struct Label {
    pub arity: u32,
    pub pc: isize,
    pub sp: usize,
}

impl Label {
    pub fn new(arity: u32, pc: isize, sp: usize) -> Self {
        Self { arity, pc, sp }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    labels: Vec<Label>,
    pub arity: u32,
    pub funcaddr: FuncAddr,
    pub locals: Vec<Value>,
    pub pc: isize,
    pub sp: usize,
}

impl Frame {
    pub fn new(arity: u32, funcaddr: FuncAddr, locals: Vec<Value>, sp: usize) -> Self {
        Self {
            labels: vec![],
            arity,
            funcaddr,
            locals,
            pc: -1,
            sp,
        }
    }

    pub fn current_label(&self) -> &Label {
        self.labels.last().unwrap()
    }

    pub fn pop_nth_label(&mut self, n: u32) -> Label {
        let n = n as usize;
        let idx = self.labels.len() - n - 1;
        let label = self.labels.swap_remove(idx);
        self.labels.truncate(idx);

        label
    }

    pub fn pop_label(&mut self) -> Option<Label> {
        self.labels.pop()
    }

    pub fn push_label(&mut self, arity: u32, pc: isize, sp: usize) {
        self.labels.push(Label::new(arity, pc, sp))
    }
}
