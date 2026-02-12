use crate::runtime::{store::FuncAddr, value::Value};

/// A Labels hold the arity of the block, the continuation (pc) and
/// the stack height (sp) for unwinding on exit.
///
/// We need two arities to handle branching on loops correctly.
/// A loop of type [t1*] -> [t2*], so when exiting the loop, on end,
/// t2 values needs to be left on the stack, but when looping
/// t1 values should be present on the stack
///
/// For other blocks, the br_arity and end_arity should be the same.
#[derive(Debug, Clone)]
pub struct Label {
    pub br_arity: u32,
    pub end_arity: u32,
    pub pc: isize,
    pub sp: usize,
}

impl Label {
    pub fn new(br_arity: u32, end_arity: u32, pc: isize, sp: usize) -> Self {
        Self {
            br_arity,
            end_arity,
            pc,
            sp,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub labels: Vec<Label>,
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

    pub fn enter_block(&mut self, arity: u32, pc: isize, sp: usize) {
        self.labels.push(Label::new(arity, arity, pc, sp))
    }
    pub fn enter_loop(&mut self, br_arity: u32, end_arity: u32, pc: isize, sp: usize) {
        self.labels.push(Label::new(br_arity, end_arity, pc, sp))
    }
}
