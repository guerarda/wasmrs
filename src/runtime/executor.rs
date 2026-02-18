use std::{
    ops::{BitAnd, BitOr, BitXor, Neg},
    result,
};

use crate::{
    binary::types::{BlockType, MemIndex},
    instructions::Instruction,
    runtime::{
        Runtime, RuntimeError,
        instance::{ModuleInstance, ModuleRegistry},
        stack::{Frame, Label},
        store::{FuncAddr, Store},
        value::{Ref, Value},
    },
};

macro_rules! unary_op {
    ($self:expr, $variant:ident, $op:expr) => {{
        let val = $self.value_stack.pop().unwrap();
        let result = match val {
            Value::$variant(a) => Value::$variant($op(a).into()),
            _ => unreachable!(),
        };
        $self.value_stack.push(result);
    }};
}

macro_rules! binary_op {
    ($self:expr, $variant:ident, $op:expr) => {{
        let rhs = $self.value_stack.pop().unwrap();
        let lhs = $self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::$variant(a), Value::$variant(b)) => $op(a, b),
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::$variant(res as _));
    }};
    ($self:expr, $ty:ty, $variant:ident, $op:expr) => {{
        let rhs = $self.value_stack.pop().unwrap();
        let lhs = $self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::$variant(a), Value::$variant(b)) => $op(a as $ty, b as $ty),
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::$variant(res as _));
    }};
}

macro_rules! comp_op {
    ($self:expr, $variant:ident, $op:expr) => {{
        let rhs = $self.value_stack.pop().unwrap();
        let lhs = $self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::$variant(a), Value::$variant(b)) => $op(a, b),
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::I32(res as _));
    }};

    ($self:expr, $ty:ty, $variant:ident, $op:expr) => {{
        let rhs = $self.value_stack.pop().unwrap();
        let lhs = $self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::$variant(a), Value::$variant(b)) => $op(a as $ty, b as $ty),
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::I32(res as _));
    }};
}

macro_rules! conv_op {
    ($self:expr, $from_variant:ident, $to_variant:ident, $op:expr) => {{
        let val = $self.value_stack.pop().unwrap();
        let res = match (val) {
            Value::$from_variant(a) => $op(a),
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::$to_variant(res));
    }};
}

macro_rules! try_binary_op {
    ($self:expr, $variant:ident, $op:expr) => {{
        let rhs = $self.value_stack.pop().unwrap();
        let lhs = $self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::$variant(a), Value::$variant(b)) => $op(a, b).ok_or(RuntimeError::trap(""))?,
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::$variant(res as _));
        Ok::<(), RuntimeError>(())
    }};

    ($self:expr, $ty:ty, $variant:ident, $op:expr) => {{
        let rhs = $self.value_stack.pop().unwrap();
        let lhs = $self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::$variant(a), Value::$variant(b)) => {
                $op(a as $ty, b as $ty).ok_or(RuntimeError::trap(""))?
            }
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::$variant(res as _));
        Ok::<(), RuntimeError>(())
    }};
}

macro_rules! load {
    ($self: expr, $module_inst: ident, $memarg: ident, $ty:ty, $variant: ident) => {{
        // Get the base address
        let i = Self::pop_i32($self.value_stack)?;

        // Read memory
        let mem = Runtime::memory_get(&$self.store.memories, $module_inst, MemIndex::ZERO).unwrap();
        let v = <$ty>::from_le_bytes(
            mem.slice(i, $memarg, std::mem::size_of::<$ty>())?
                .try_into()
                .unwrap(),
        );

        // Push value
        $self.value_stack.push(Value::$variant(v as _));
    }};
}

macro_rules! store {
    ($self: expr, $module_inst: ident, $memarg: ident, $variant: ident, $ty:ty) => {{
        // Get the value
        let v = match $self.value_stack.pop().unwrap() {
            Value::$variant(val) => val as $ty,
            _ => unreachable!(),
        };

        // Get the base address
        let i = Self::pop_i32($self.value_stack)?;

        // Get memory
        let slice =
            Runtime::memory_get_mut(&mut $self.store.memories, $module_inst, MemIndex::ZERO)
                .unwrap()
                .slice_mut(i, $memarg, std::mem::size_of::<$ty>())?;

        // Store
        slice.copy_from_slice(&v.to_le_bytes());
    }};
}

pub(super) struct ExecutionContext<'a> {
    value_stack: &'a mut Vec<Value>,
    call_stack: &'a mut Vec<Frame>,
    store: &'a mut Store,
    module_registry: &'a ModuleRegistry,
}

impl<'a> ExecutionContext<'a> {
    pub(super) fn new(
        value_stack: &'a mut Vec<Value>,
        call_stack: &'a mut Vec<Frame>,
        store: &'a mut Store,
        module_registry: &'a ModuleRegistry,
    ) -> Self {
        Self {
            value_stack,
            call_stack,
            store,
            module_registry,
        }
    }
    /// Find the index of the 'end' instruction for the block at idx
    fn find_block_end(instrs: &[Instruction], mut idx: isize) -> isize {
        debug_assert!(
            matches!(
                instrs.get(idx as usize),
                Some(Instruction::Block(_) | Instruction::Loop(_))
            ),
            "find_block_end must be called with idx pointing at a 'block' or 'loop' instruction"
        );

        let mut depth = 0;
        while let Some(instr) = instrs.get(idx as usize) {
            match instr {
                Instruction::Block(_) | Instruction::Loop(_) | Instruction::If(_) => {
                    depth += 1;
                }
                Instruction::End => {
                    depth -= 1;
                }
                _ => (),
            }
            if depth == 0 {
                return idx;
            }
            idx += 1;
        }
        panic!("block without matching end")
    }

    /// Find the 'end' and 'else' matching the 'if' instruction at idx
    fn find_if_else_end(instrs: &[Instruction], mut idx: isize) -> (isize, Option<isize>) {
        debug_assert!(
            matches!(instrs.get(idx as usize), Some(Instruction::If(_))),
            "find_else_or_end must be called with idx pointing at a 'if' instruction"
        );

        let mut depth = 0;
        let mut else_ = None;

        while let Some(instr) = instrs.get(idx as usize) {
            match instr {
                Instruction::Block(_) | Instruction::Loop(_) | Instruction::If(_) => {
                    depth += 1;
                }
                Instruction::End => {
                    depth -= 1;
                }
                Instruction::Else if depth == 1 => else_ = Some(idx),
                _ => (),
            }
            if depth == 0 {
                return (idx, else_);
            }
            idx += 1;
        }
        panic!("block without matching end")
    }

    /// Returns the numbers of parameters and results for the block
    fn block_arity(bt: &BlockType, module_inst: &ModuleInstance) -> (u32, u32) {
        match bt {
            BlockType::Empty => (0, 0),
            BlockType::Value(_) => (0, 1),
            BlockType::Index(idx) => {
                let ftype = &module_inst.types[*idx as usize];
                (ftype.params.len() as u32, ftype.results.len() as u32)
            }
        }
    }

    fn unwind_value_stack(
        value_stack: &mut Vec<Value>,
        sp: usize,
        arity: u32,
    ) -> result::Result<(), RuntimeError> {
        let arity = arity as usize;

        let stack_len = value_stack.len();
        if stack_len < arity {
            return Err(RuntimeError::internal(
                "unwinding {arity} value from stack with len {stack_len}",
            ));
        }

        let results_idx = stack_len - arity;
        if results_idx < sp {
            return Err(RuntimeError::internal("unwinding past stack pointer"));
        }
        // Rotate the results down to sp, then truncate
        value_stack[sp..].rotate_left(results_idx - sp);
        value_stack.truncate(sp + arity);

        Ok(())
    }

    fn branch(
        value_stack: &mut Vec<Value>,
        call_stack: &mut Vec<Frame>,
        label_idx: &u32,
    ) -> result::Result<(), RuntimeError> {
        let frame = call_stack.last_mut().unwrap();
        let n = *label_idx as usize;

        if n == frame.labels.len() {
            // function return
            Self::unwind_value_stack(value_stack, frame.sp, frame.arity)?;
            call_stack.pop();
            return Ok(());
        }

        let label = frame.pop_nth_label(*label_idx);
        Self::unwind_value_stack(value_stack, label.sp, label.br_arity)?;
        frame.pc = label.pc;
        Ok(())
    }

    fn pop_i32(value_stack: &mut Vec<Value>) -> result::Result<i32, RuntimeError> {
        value_stack
            .pop()
            .and_then(Value::as_i32)
            .ok_or_else(|| RuntimeError::internal("assert, i32 expected on the stack"))
    }

    fn pop_bool(value_stack: &mut Vec<Value>) -> result::Result<bool, RuntimeError> {
        value_stack
            .pop()
            .and_then(Value::as_bool)
            .ok_or_else(|| RuntimeError::internal("assert, i32 expected on the stack"))
    }

    pub(super) fn call(&mut self, funcaddr: FuncAddr) {
        let func_instance = &self.store.functions.get(funcaddr);
        let n_args = func_instance.ftype.params.len();
        let arity = func_instance.ftype.results.len() as u32;
        let sp = self.value_stack.len() - n_args;

        let mut locals: Vec<Value> = self.value_stack.split_off(sp);
        locals.extend(
            func_instance
                .func
                .locals
                .clone()
                .into_iter()
                .map(Into::<Value>::into),
        );
        self.call_stack
            .push(Frame::new(arity, funcaddr, locals, sp));
    }

    pub(super) fn execute(&mut self) -> result::Result<(), RuntimeError> {
        while let Some(frame) = self.call_stack.last_mut() {
            let func_inst = self.store.functions.get(frame.funcaddr);
            let module_inst = self.module_registry.get_instance(func_inst.module);
            let instrs = &func_inst.func.body;

            frame.pc += 1;
            if let Some(inst) = instrs.get(frame.pc as usize) {
                match inst {
                    Instruction::Unreachable => {
                        return Err(RuntimeError::trap("unreachable"));
                    }
                    Instruction::Nop => continue,
                    Instruction::Block(bt) => {
                        let (n_params, n_results) = Self::block_arity(bt, module_inst);
                        let end = Self::find_block_end(instrs, frame.pc);
                        frame.enter_block(
                            n_results,
                            end,
                            self.value_stack.len() - n_params as usize,
                        );
                    }
                    Instruction::Loop(bt) => {
                        let (n_params, n_results) = Self::block_arity(bt, module_inst);
                        frame.enter_loop(
                            n_params,
                            n_results,
                            frame.pc - 1, // Make sure we create the label on every loop
                            self.value_stack.len() - n_params as usize,
                        );
                    }
                    Instruction::If(bt) => {
                        let (n_params, n_results) = Self::block_arity(bt, module_inst);
                        let (end, else_) = Self::find_if_else_end(instrs, frame.pc);

                        let cond = Self::pop_bool(self.value_stack)?;
                        frame.enter_block(
                            n_results,
                            end,
                            self.value_stack.len() - n_params as usize,
                        );

                        if !cond {
                            // Next instruction is one past else or end
                            frame.pc = else_.unwrap_or(end - 1);
                        }
                    }
                    Instruction::Else => {
                        frame.pc = frame.current_label().pc - 1;
                    }
                    Instruction::End => match frame.labels.pop() {
                        Some(Label { end_arity, sp, .. }) => {
                            Self::unwind_value_stack(self.value_stack, sp, end_arity)?;
                        }
                        None => {
                            Self::unwind_value_stack(self.value_stack, frame.sp, frame.arity)?;
                            self.call_stack.pop();
                        }
                    },
                    Instruction::Br(label_idx) => {
                        Self::branch(self.value_stack, self.call_stack, label_idx)?;
                    }
                    Instruction::BrIf(label_idx) => {
                        let cond = Self::pop_bool(self.value_stack)?;
                        if cond {
                            Self::branch(self.value_stack, self.call_stack, label_idx)?;
                        }
                    }
                    Instruction::BrTable(br_idx) => {
                        let i = Self::pop_i32(self.value_stack)? as usize;

                        let label_idx = br_idx.labels.get(i).unwrap_or(&br_idx.default);
                        Self::branch(self.value_stack, self.call_stack, label_idx)?;
                    }
                    Instruction::Return => {
                        Self::unwind_value_stack(self.value_stack, frame.sp, frame.arity)?;
                        self.call_stack.pop();
                    }
                    Instruction::Call(idx) => {
                        let funcaddr = module_inst.funcs[*idx as usize];
                        self.call(funcaddr);
                    }
                    Instruction::CallIndirect((table_idx, type_idx)) => {
                        let tab_inst =
                            Runtime::table_get(&self.store.tables, module_inst, *table_idx)?;
                        let ft = &module_inst.types[*type_idx as usize];

                        let i = Self::pop_i32(self.value_stack)?;

                        let r = tab_inst
                            .refs
                            .get(i as usize)
                            .ok_or(RuntimeError::trap("tab index out of bounds"))?;

                        if r.is_null() {
                            return Err(RuntimeError::trap("unexpected null ref"));
                        }

                        let func_addr = r
                            .as_funcref()
                            .ok_or(RuntimeError::internal("assert: expected func ref"))?;
                        if self.store.functions.get(func_addr).ftype != *ft {
                            return Err(RuntimeError::trap("function type mismatch"));
                        }
                        self.call(func_addr);
                    }
                    Instruction::Drop => {
                        self.value_stack.pop();
                    }

                    Instruction::Select | Instruction::SelectT(_) => {
                        let cond = Self::pop_bool(self.value_stack)?;
                        let val2 = self.value_stack.pop().unwrap();
                        let val1 = self.value_stack.pop().unwrap();

                        if cond {
                            self.value_stack.push(val1);
                        } else {
                            self.value_stack.push(val2);
                        }
                    }
                    Instruction::LocalGet(idx) => {
                        let idx = *idx as usize;
                        let v = frame
                            .locals
                            .get(idx)
                            .ok_or_else(|| RuntimeError::internal("local index out of bounds"))?;
                        self.value_stack.push(*v)
                    }
                    Instruction::LocalSet(idx) => {
                        let v = self.value_stack.pop().ok_or_else(|| {
                            RuntimeError::internal("assert, value expected on the stack")
                        })?;
                        let idx = *idx as usize;
                        *frame
                            .locals
                            .get_mut(idx)
                            .ok_or_else(|| RuntimeError::internal("local index out of bounds"))? =
                            v;
                    }
                    Instruction::LocalTee(idx) => {
                        let v = self.value_stack.last().ok_or_else(|| {
                            RuntimeError::internal("assert, value expected on the stack")
                        })?;
                        let idx = *idx as usize;
                        *frame
                            .locals
                            .get_mut(idx)
                            .ok_or_else(|| RuntimeError::internal("local index out of bounds"))? =
                            *v;
                    }
                    Instruction::GlobalGet(idx) => {
                        let v = Runtime::global_get(&self.store.globals, module_inst, *idx)?;
                        self.value_stack.push(v);
                    }
                    Instruction::GlobalSet(idx) => {
                        let v = self.value_stack.pop().unwrap();
                        Runtime::global_set(&mut self.store.globals, module_inst, *idx, v)?;
                    }
                    Instruction::I32Load(memarg) => load!(self, module_inst, memarg, i32, I32),
                    Instruction::I64Load(memarg) => {
                        load!(self, module_inst, memarg, i64, I64);
                    }
                    Instruction::F32Load(memarg) => {
                        load!(self, module_inst, memarg, f32, F32);
                    }
                    Instruction::F64Load(memarg) => {
                        load!(self, module_inst, memarg, f64, F64);
                    }
                    Instruction::I32Load8S(memarg) => {
                        load!(self, module_inst, memarg, i8, I32);
                    }
                    Instruction::I32Load8U(memarg) => {
                        load!(self, module_inst, memarg, u8, I32);
                    }
                    Instruction::I32Load16S(memarg) => {
                        load!(self, module_inst, memarg, i16, I32);
                    }
                    Instruction::I32Load16U(memarg) => {
                        load!(self, module_inst, memarg, u16, I32);
                    }
                    Instruction::I64Load8S(memarg) => {
                        load!(self, module_inst, memarg, i8, I64);
                    }
                    Instruction::I64Load8U(memarg) => {
                        load!(self, module_inst, memarg, u8, I64);
                    }
                    Instruction::I64Load16S(memarg) => {
                        load!(self, module_inst, memarg, i16, I64);
                    }
                    Instruction::I64Load16U(memarg) => {
                        load!(self, module_inst, memarg, u16, I64);
                    }
                    Instruction::I64Load32S(memarg) => {
                        load!(self, module_inst, memarg, i32, I64);
                    }
                    Instruction::I64Load32U(memarg) => {
                        load!(self, module_inst, memarg, u32, I64);
                    }
                    Instruction::I32Store(memarg) => {
                        store!(self, module_inst, memarg, I32, i32);
                    }
                    Instruction::I64Store(memarg) => {
                        store!(self, module_inst, memarg, I64, i64);
                    }
                    Instruction::F32Store(memarg) => {
                        store!(self, module_inst, memarg, F32, f32);
                    }
                    Instruction::F64Store(memarg) => {
                        store!(self, module_inst, memarg, F64, f64);
                    }
                    Instruction::I32Store8(memarg) => {
                        store!(self, module_inst, memarg, I32, u8);
                    }
                    Instruction::I32Store16(memarg) => {
                        store!(self, module_inst, memarg, I32, u16);
                    }
                    Instruction::I64Store8(memarg) => {
                        store!(self, module_inst, memarg, I64, u8);
                    }
                    Instruction::I64Store16(memarg) => {
                        store!(self, module_inst, memarg, I64, u16);
                    }
                    Instruction::I64Store32(memarg) => {
                        store!(self, module_inst, memarg, I64, u32);
                    }
                    Instruction::MemorySize(idx) => {
                        let mem =
                            Runtime::memory_get(&self.store.memories, module_inst, *idx).unwrap();
                        let sz = mem.size();
                        self.value_stack.push(Value::I32(sz as i32));
                    }
                    Instruction::MemoryGrow(idx) => {
                        let inc =
                            self.value_stack.pop().and_then(Value::as_i32).ok_or(
                                RuntimeError::internal("memory.grow, invalid argument type"),
                            )?;
                        let res =
                            Runtime::memory_get_mut(&mut self.store.memories, module_inst, *idx)
                                .unwrap()
                                .grow(inc as u32)?
                                .map(|v| v as i32)
                                .unwrap_or(-1);
                        self.value_stack.push(Value::I32(res));
                    }

                    Instruction::I32Const(v) => self.value_stack.push(Value::I32(*v)),
                    Instruction::I64Const(v) => self.value_stack.push(Value::I64(*v)),
                    Instruction::F32Const(v) => self.value_stack.push(Value::F32(*v)),
                    Instruction::F64Const(v) => self.value_stack.push(Value::F64(*v)),

                    // Comparison ops
                    Instruction::I32Eqz => unary_op!(self, I32, |a| a == 0),
                    Instruction::I32Eq => comp_op!(self, I32, |a, b| a == b),
                    Instruction::I32Ne => comp_op!(self, I32, |a, b| a != b),
                    Instruction::I32LtS => comp_op!(self, I32, |a, b| a < b),
                    Instruction::I32LtU => comp_op!(self, u32, I32, |a, b| a < b),
                    Instruction::I32GtS => comp_op!(self, I32, |a, b| a > b),
                    Instruction::I32GtU => comp_op!(self, u32, I32, |a, b| a > b),
                    Instruction::I32LeS => comp_op!(self, I32, |a, b| a <= b),
                    Instruction::I32LeU => comp_op!(self, u32, I32, |a, b| a <= b),
                    Instruction::I32GeS => comp_op!(self, I32, |a, b| a >= b),
                    Instruction::I32GeU => comp_op!(self, u32, I32, |a, b| a >= b),

                    Instruction::I64Eqz => {
                        let val = self.value_stack.pop().unwrap();
                        let result = match val {
                            Value::I64(a) => Value::I32((a == 0) as i32),
                            _ => unreachable!(),
                        };
                        self.value_stack.push(result);
                    }
                    Instruction::I64Eq => comp_op!(self, I64, |a, b| a == b),
                    Instruction::I64Ne => comp_op!(self, I64, |a, b| a != b),
                    Instruction::I64LtS => comp_op!(self, I64, |a, b| a < b),
                    Instruction::I64LtU => comp_op!(self, u64, I64, |a, b| a < b),
                    Instruction::I64GtS => comp_op!(self, I64, |a, b| a > b),
                    Instruction::I64GtU => comp_op!(self, u64, I64, |a, b| a > b),
                    Instruction::I64LeS => comp_op!(self, I64, |a, b| a <= b),
                    Instruction::I64LeU => comp_op!(self, u64, I64, |a, b| a <= b),
                    Instruction::I64GeS => comp_op!(self, I64, |a, b| a >= b),
                    Instruction::I64GeU => comp_op!(self, u64, I64, |a, b| a >= b),

                    Instruction::F32Eq => comp_op!(self, F32, |a, b| a == b),
                    Instruction::F32Ne => comp_op!(self, F32, |a, b| a != b),
                    Instruction::F32Lt => comp_op!(self, F32, |a, b| a < b),
                    Instruction::F32Gt => comp_op!(self, F32, |a, b| a > b),
                    Instruction::F32Le => comp_op!(self, F32, |a, b| a <= b),
                    Instruction::F32Ge => comp_op!(self, F32, |a, b| a >= b),

                    Instruction::F64Le => comp_op!(self, F64, |a, b| a <= b),

                    // Unary ops
                    Instruction::I32Clz => unary_op!(self, I32, |a: i32| a.leading_zeros() as i32),
                    Instruction::I32Ctz => unary_op!(self, I32, |a: i32| a.trailing_zeros() as i32),
                    Instruction::I32Popcnt => unary_op!(self, I32, |a: i32| a.count_ones() as i32),

                    // Arithmetic ops
                    Instruction::I32Add => binary_op!(self, I32, |a: i32, b| a.wrapping_add(b)),
                    Instruction::I32Sub => binary_op!(self, I32, |a: i32, b| a.wrapping_sub(b)),
                    Instruction::I32Mul => binary_op!(self, I32, |a: i32, b| a.wrapping_mul(b)),
                    Instruction::I32DivS => {
                        try_binary_op!(self, I32, |a: i32, b| a.checked_div(b))?
                    }

                    Instruction::I32DivU => {
                        try_binary_op!(self, u32, I32, |a: u32, b| a.checked_div(b))?
                    }
                    Instruction::I32RemS => try_binary_op!(self, I32, |a: i32, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,
                    Instruction::I32RemU => try_binary_op!(self, u32, I32, |a: u32, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,
                    Instruction::I32And => binary_op!(self, I32, BitAnd::bitand),
                    Instruction::I32Or => binary_op!(self, I32, BitOr::bitor),
                    Instruction::I32Xor => binary_op!(self, I32, BitXor::bitxor),
                    Instruction::I32Shl => {
                        binary_op!(self, I32, |a: i32, b| a.wrapping_shl(b as u32))
                    }
                    Instruction::I32ShrS => {
                        binary_op!(self, I32, |a: i32, b| a.wrapping_shr(b as u32))
                    }
                    Instruction::I32ShrU => {
                        binary_op!(self, u32, I32, |a: u32, b| a.wrapping_shr(b))
                    }
                    Instruction::I32Rotl => {
                        binary_op!(self, I32, |a: i32, b| a.rotate_left(b as u32))
                    }
                    Instruction::I32Rotr => {
                        binary_op!(self, I32, |a: i32, b| a.rotate_right(b as u32))
                    }

                    Instruction::I64Clz => unary_op!(self, I64, |a: i64| a.leading_zeros() as i64),
                    Instruction::I64Ctz => unary_op!(self, I64, |a: i64| a.trailing_zeros() as i64),
                    Instruction::I64Popcnt => {
                        unary_op!(self, I64, |a: i64| a.count_ones() as i64)
                    }

                    Instruction::I64Add => {
                        binary_op!(self, I64, |a: i64, b: i64| a.wrapping_add(b))
                    }
                    Instruction::I64Sub => binary_op!(self, I64, |a: i64, b| a.wrapping_sub(b)),
                    Instruction::I64Mul => binary_op!(self, I64, |a: i64, b| a.wrapping_mul(b)),
                    Instruction::I64DivS => {
                        try_binary_op!(self, I64, |a: i64, b| a.checked_div(b))?
                    }
                    Instruction::I64DivU => {
                        try_binary_op!(self, u64, I64, |a: u64, b| a.checked_div(b))?
                    }

                    Instruction::I64RemS => try_binary_op!(self, I64, |a: i64, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,
                    Instruction::I64RemU => try_binary_op!(self, u64, I64, |a: u64, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,
                    Instruction::I64And => binary_op!(self, I64, BitAnd::bitand),
                    Instruction::I64Or => binary_op!(self, I64, BitOr::bitor),
                    Instruction::I64Xor => binary_op!(self, I64, BitXor::bitxor),
                    Instruction::I64Shl => {
                        binary_op!(self, I64, |a: i64, b| a.wrapping_shl(b as u32))
                    }
                    Instruction::I64ShrS => {
                        binary_op!(self, I64, |a: i64, b| a.wrapping_shr(b as u32))
                    }
                    Instruction::I64ShrU => {
                        binary_op!(self, I64, |a: i64, b| (a as u64).wrapping_shr(b as u32)
                            as i64)
                    }
                    Instruction::I64Rotl => {
                        binary_op!(self, I64, |a: i64, b| a.rotate_left(b as u32))
                    }
                    Instruction::I64Rotr => {
                        binary_op!(self, I64, |a: i64, b| a.rotate_right(b as u32))
                    }

                    Instruction::F32Abs => unary_op!(self, F32, |a: f32| a.abs()),
                    Instruction::F32Neg => unary_op!(self, F32, |a: f32| a.neg()),

                    Instruction::F32Ceil => unary_op!(self, F32, |a: f32| a.ceil()),
                    Instruction::F32Floor => unary_op!(self, F32, |a: f32| a.floor()),

                    Instruction::F32Trunc => unary_op!(self, F32, |a: f32| a.trunc()),
                    Instruction::F32Nearest => unary_op!(self, F32, |a: f32| a.round_ties_even()),
                    Instruction::F32Sqrt => unary_op!(self, F32, |a: f32| a.sqrt()),
                    Instruction::F32Add => binary_op!(self, F32, |a: f32, b: f32| a + b),
                    Instruction::F32Sub => binary_op!(self, F32, |a: f32, b: f32| a - b),
                    Instruction::F32Mul => binary_op!(self, F32, |a: f32, b: f32| a * b),
                    Instruction::F32Div => binary_op!(self, F32, |a: f32, b: f32| a / b),
                    Instruction::F32Min => binary_op!(self, F32, |a: f32, b: f32| a.min(b)),
                    Instruction::F32Max => binary_op!(self, F32, |a: f32, b: f32| a.max(b)),
                    Instruction::F32Copysign => {
                        binary_op!(self, F32, |a: f32, b: f32| a.copysign(b))
                    }

                    Instruction::F64Neg => {
                        unary_op!(self, F64, |a: f64| -a);
                    }
                    Instruction::F64Floor => {
                        unary_op!(self, F64, |a: f64| a.floor());
                    }
                    Instruction::F64Add => {
                        binary_op!(self, F64, |a: f64, b: f64| a + b);
                    }
                    // Conversion ops
                    Instruction::I32WrapI64 => {
                        conv_op!(self, I64, I32, |a| a as i32);
                    }
                    Instruction::I64ExtendI32S => {
                        conv_op!(self, I32, I64, i64::from)
                    }
                    Instruction::I64ExtendI32U => {
                        conv_op!(self, I32, I64, |a| a as u32 as i64);
                    }

                    // Sign extension ops
                    Instruction::I32Extend8S => conv_op!(self, I32, I32, |a| a as i8 as i32),
                    Instruction::I32Extend16S => conv_op!(self, I32, I32, |a| a as i16 as i32),
                    Instruction::I64Extend8S => conv_op!(self, I64, I64, |a| a as i8 as i64),
                    Instruction::I64Extend16S => conv_op!(self, I64, I64, |a| a as i16 as i64),
                    Instruction::I64Extend32S => conv_op!(self, I64, I64, |a| a as i32 as i64),

                    // Ref
                    Instruction::RefNull(rt) => self.value_stack.push(Value::Ref(Ref::Null(*rt))),
                    Instruction::RefFunc(fi) => {
                        self.value_stack.push(Value::Ref(Ref::Func((*fi).into())))
                    }
                    Instruction::I32TruncSatF32S => todo!(),
                    Instruction::I32TruncSatF32U => todo!(),
                    Instruction::I64TruncSatF64S => todo!(),
                    Instruction::I64TruncSatF64U => todo!(),

                    Instruction::MemoryInit(_) => todo!(),
                    Instruction::DataDrop(idx) => {
                        let da = module_inst.datas[*idx as usize];
                        self.store.data.drop(da);
                    }
                    Instruction::TableInit(_) => todo!(),
                    Instruction::ElemDrop(idx) => {
                        let ea = module_inst.elems[*idx as usize];
                        self.store.elements.drop(ea);
                    }
                }
            }
        }
        Ok(())
    }
}

impl Runtime {}

#[cfg(test)]
mod tests {
    use crate::runtime::{Runtime, value::Value};

    #[test]
    fn test_memory_size() -> anyhow::Result<()> {
        // (module
        //   (memory 1)
        //   (func (export "size") (result i32) memory.size))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, () -> (i32)
            b"\x01\x05\x01\x60\x00\x01\x7f",
            // function section: 1 func, type 0
            b"\x03\x02\x01\x00",
            // memory section: 1 memory, min=1 page
            b"\x05\x03\x01\x00\x01",
            // export section: "size" -> func 0
            b"\x07\x08\x01\x04size\x00\x00",
            // code section: memory.size 0, end
            b"\x0a\x06\x01\x04\x00\x3f\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let r = runtime.invoke(mh, "size", &[])?;
        assert!(matches!(r.as_slice(), [Value::I32(1)]));
        assert_eq!(runtime.memory_pages(0), 1);

        Ok(())
    }

    #[test]
    fn test_memory_grow() -> anyhow::Result<()> {
        // (module
        //   (memory 1)
        //   (func (export "grow") (param i32) (result i32) local.get 0 memory.grow)
        //   (func (export "size") (result i32) memory.size))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 2 types
            //   type 0: (i32) -> (i32)
            //   type 1: () -> (i32)
            b"\x01\x0a\x02\x60\x01\x7f\x01\x7f\x60\x00\x01\x7f",
            // function section: 2 funcs, type 0 and type 1
            b"\x03\x03\x02\x00\x01",
            // memory section: 1 memory, min=1 page
            b"\x05\x03\x01\x00\x01",
            // export section: "grow" -> func 0, "size" -> func 1
            b"\x07\x0f\x02\x04grow\x00\x00\x04size\x00\x01",
            // code section: 2 entries
            //   func 0: local.get 0, memory.grow 0, end
            //   func 1: memory.size 0, end
            b"\x0a\x0d\x02\x06\x00\x20\x00\x40\x00\x0b\x04\x00\x3f\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        // Initial size is 1 page
        let r = runtime.invoke(mh, "size", &[])?;
        assert!(matches!(r.as_slice(), [Value::I32(1)]));
        assert_eq!(runtime.memory_pages(0), 1);

        // Grow by 2 pages, returns old size (1)
        let r = runtime.invoke(mh, "grow", &[Value::I32(2)])?;
        assert!(matches!(r.as_slice(), [Value::I32(1)]));
        assert_eq!(runtime.memory_pages(0), 3);
        assert_eq!(runtime.memory_data(0).len(), 3 * 65536);

        // New size is 3 pages
        let r = runtime.invoke(mh, "size", &[])?;
        assert!(matches!(r.as_slice(), [Value::I32(3)]));

        Ok(())
    }

    #[test]
    fn test_i32_store_load() -> anyhow::Result<()> {
        // (module
        //   (memory 1)
        //   (func (export "store") (param i32 i32) local.get 0 local.get 1 i32.store)
        //   (func (export "load") (param i32) (result i32) local.get 0 i32.load))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 2 types
            //   type 0: (i32 i32) -> ()
            //   type 1: (i32) -> (i32)
            b"\x01\x0b\x02\x60\x02\x7f\x7f\x00\x60\x01\x7f\x01\x7f",
            // function section: 2 funcs, type 0 and type 1
            b"\x03\x03\x02\x00\x01",
            // memory section: 1 memory, min=1 page
            b"\x05\x03\x01\x00\x01",
            // export section: "store" -> func 0, "load" -> func 1
            b"\x07\x10\x02\x05store\x00\x00\x04load\x00\x01",
            // code section: 2 entries
            //   func 0: local.get 0, local.get 1, i32.store align=2 offset=0, end
            //   func 1: local.get 0, i32.load align=2 offset=0, end
            b"\x0a\x13\x02\x09\x00\x20\x00\x20\x01\x36\x02\x00\x0b\x07\x00\x20\x00\x28\x02\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        // Store 42 at address 0
        runtime.invoke(mh, "store", &[Value::I32(0), Value::I32(42)])?;
        assert_eq!(&runtime.memory_data(0)[0..4], &42_i32.to_le_bytes());

        // Load from address 0
        let r = runtime.invoke(mh, "load", &[Value::I32(0)])?;
        assert!(matches!(r.as_slice(), [Value::I32(42)]));

        // Store at a different offset and load it back
        runtime.invoke(mh, "store", &[Value::I32(100), Value::I32(-1)])?;
        assert_eq!(&runtime.memory_data(0)[100..104], &(-1_i32).to_le_bytes());

        let r = runtime.invoke(mh, "load", &[Value::I32(100)])?;
        assert!(matches!(r.as_slice(), [Value::I32(-1)]));

        Ok(())
    }

    #[test]
    fn test_if_param() -> anyhow::Result<()> {
        // (module
        //   (func (export "param") (param i32) (result i32)
        //     (i32.const 1)
        //     (if (param i32) (result i32) (local.get 0)
        //       (then (i32.const 2) (i32.add))
        //       (else (i32.const -2) (i32.add))))
        //   (func (export "params") (param i32) (result i32)
        //     (i32.const 1) (i32.const 2)
        //     (if (param i32 i32) (result i32) (local.get 0)
        //       (then (i32.add))
        //       (else (i32.sub))))
        //   (func (export "params-id") (param i32) (result i32)
        //     (i32.const 1) (i32.const 2)
        //     (if (param i32 i32) (result i32 i32) (local.get 0) (then))
        //     (i32.add)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 3 types
            //   type 0: (i32) -> (i32)
            //   type 1: (i32 i32) -> (i32)
            //   type 2: (i32 i32) -> (i32 i32)
            b"\x01\x13\x03\x60\x01\x7f\x01\x7f\x60\x02\x7f\x7f\x01\x7f\x60\x02\x7f\x7f\x02\x7f\x7f",
            // function section: 3 funcs, all type 0
            b"\x03\x04\x03\x00\x00\x00",
            // export section: "param" -> func 0, "params" -> func 1, "params-id" -> func 2
            b"\x07\x1e\x03\x05param\x00\x00\x06params\x00\x01\x09params-id\x00\x02",
            // code section: 3 entries
            b"\x0a\x2e\x03",
            // func 0: i32.const 1, local.get 0, if(type 0), i32.const 2, i32.add, else, i32.const -2, i32.add, end, end
            b"\x10\x00\x41\x01\x20\x00\x04\x00\x41\x02\x6a\x05\x41\x7e\x6a\x0b\x0b",
            // func 1: i32.const 1, i32.const 2, local.get 0, if(type 1), i32.add, else, i32.sub, end, end
            b"\x0e\x00\x41\x01\x41\x02\x20\x00\x04\x01\x6a\x05\x6b\x0b\x0b",
            // func 2: i32.const 1, i32.const 2, local.get 0, if(type 2), end, i32.add, end
            b"\x0c\x00\x41\x01\x41\x02\x20\x00\x04\x02\x0b\x6a\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let cases = [
            ("param", 0, -1),
            ("param", 1, 3),
            ("params", 0, -1),
            ("params", 1, 3),
            ("params-id", 0, 3),
            ("params-id", 1, 3),
        ];

        for (name, arg, expected) in cases {
            let r = runtime.invoke(mh, name, &[Value::I32(arg)])?;
            assert!(
                matches!(r.as_slice(), [Value::I32(v)] if *v == expected),
                "{name}({arg}): expected {expected}, got {r:?}",
            );
        }

        Ok(())
    }

    #[test]
    fn test_call_indirect() -> anyhow::Result<()> {
        // (module
        //   (type $t (func (param i32 i32) (result i32)))
        //   (table 2 funcref)
        //   (elem (i32.const 0) func $add $sub)
        //   (func $add (type $t) local.get 0 local.get 1 i32.add)
        //   (func $sub (type $t) local.get 0 local.get 1 i32.sub)
        //   (func (export "call") (param i32 i32 i32) (result i32)
        //     local.get 0 local.get 1 local.get 2 call_indirect (type $t)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 2 types — (i32,i32)->(i32) and (i32,i32,i32)->(i32)
            b"\x01\x0e\x02\x60\x02\x7f\x7f\x01\x7f\x60\x03\x7f\x7f\x7f\x01\x7f",
            // function section: 3 funcs — type 0, type 0, type 1
            b"\x03\x04\x03\x00\x00\x01",
            // table section: 1 table, funcref, min=2
            b"\x04\x04\x01\x70\x00\x02",
            // export section: "call" -> func 2
            b"\x07\x08\x01\x04\x63\x61\x6c\x6c\x00\x02",
            // element section: active, table 0, offset=0, 2 funcs [0, 1]
            b"\x09\x08\x01\x00\x41\x00\x0b\x02\x00\x01",
            // code section: 3 funcs
            b"\x0a\x1d\x03\x07\x00\x20\x00\x20\x01\x6a\x0b\x07\x00\x20\x00\x20\x01\x6b\x0b\x0b\x00\x20\x00\x20\x01\x20\x02\x11\x00\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let cases = [
            // (a, b, table_idx, expected)
            (10, 3, 0, 13), // add
            (10, 3, 1, 7),  // sub
        ];

        for (a, b, idx, expected) in cases {
            let r = runtime.invoke(mh, "call", &[Value::I32(a), Value::I32(b), Value::I32(idx)])?;
            assert!(
                matches!(r.as_slice(), [Value::I32(v)] if *v == expected),
                "call({a}, {b}, {idx}): expected {expected}, got {r:?}",
            );
        }

        Ok(())
    }

    #[test]
    fn test_global_get() -> anyhow::Result<()> {
        // (module
        //   (global $g (mut i32) (i32.const 42))
        //   (func (export "get") (result i32) global.get $g))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, () -> (i32)
            b"\x01\x05\x01\x60\x00\x01\x7f",
            // function section: 1 func, type 0
            b"\x03\x02\x01\x00",
            // global section: 1 global, i32 mut, init=i32.const 42
            b"\x06\x06\x01\x7f\x01\x41\x2a\x0b",
            // export section: "get" -> func 0
            b"\x07\x07\x01\x03\x67\x65\x74\x00\x00",
            // code section: global.get 0, end
            b"\x0a\x06\x01\x04\x00\x23\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        // Init expression is (i32.const 42), so get must return 42
        let r = runtime.invoke(mh, "get", &[])?;
        assert!(
            matches!(r.as_slice(), [Value::I32(42)]),
            "expected 42 from init expr, got {r:?}",
        );

        Ok(())
    }

    #[test]
    fn test_br_to_function_return() -> anyhow::Result<()> {
        // (module
        //   (func (export "test") (result i32)
        //     (loop
        //       (i32.const 42)
        //       (br 1)
        //     )
        //     (unreachable)))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, () -> (i32)
            b"\x01\x05\x01\x60\x00\x01\x7f",
            // function section: 1 func, type 0
            b"\x03\x02\x01\x00",
            // export section: "test" -> func 0
            b"\x07\x08\x01\x04\x74\x65\x73\x74\x00\x00",
            // code section: loop, i32.const 42, br 1, end, unreachable, end
            b"\x0a\x0c\x01\x0a\x00\x03\x40\x41\x2a\x0c\x01\x0b\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let r = runtime.invoke(mh, "test", &[])?;
        assert!(
            matches!(r.as_slice(), [Value::I32(42)]),
            "expected 42 from br 1 (return), got {r:?}",
        );

        Ok(())
    }

    #[test]
    fn test_loop_result_arity() -> anyhow::Result<()> {
        // (module
        //   (func (export "test") (result i32)
        //     (loop (result i32)
        //       (i32.const 42)
        //     )))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, () -> (i32)
            b"\x01\x05\x01\x60\x00\x01\x7f",
            // function section: 1 func, type 0
            b"\x03\x02\x01\x00",
            // export section: "test" -> func 0
            b"\x07\x08\x01\x04\x74\x65\x73\x74\x00\x00",
            // code section: loop (result i32), i32.const 42, end, end
            b"\x0a\x09\x01\x07\x00\x03\x7f\x41\x2a\x0b\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let r = runtime.invoke(mh, "test", &[])?;
        assert!(
            matches!(r.as_slice(), [Value::I32(42)]),
            "expected 42 from loop fallthrough, got {r:?}",
        );

        Ok(())
    }

    #[test]
    fn test_loop_br_reentry() -> anyhow::Result<()> {
        // (module
        //   (func (export "test") (result i32)
        //     (local $i i32)
        //     (block $exit (result i32)
        //       (loop $cont (result i32)
        //         (local.set $i (i32.add (local.get $i) (i32.const 1)))
        //         (if (i32.eq (local.get $i) (i32.const 3))
        //           (then (br $exit (local.get $i)))
        //         )
        //         (br $cont)
        //       )
        //     )
        //   ))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 1 type, () -> (i32)
            b"\x01\x05\x01\x60\x00\x01\x7f",
            // function section: 1 func, type 0
            b"\x03\x02\x01\x00",
            // export section: "test" -> func 0
            b"\x07\x08\x01\x04test\x00\x00",
            // code section
            b"\x0a\x21\x01\x1f\x01\x01\x7f\x02\x7f\x03\x7f\x20\x00\x41\x01\x6a\x21\x00\x20\x00\x41\x03\x46\x04\x40\x20\x00\x0c\x02\x0b\x0c\x00\x0b\x0b\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        let r = runtime.invoke(mh, "test", &[])?;
        assert!(
            matches!(r.as_slice(), [Value::I32(3)]),
            "expected 3 from loop counter, got {r:?}",
        );

        Ok(())
    }

    #[test]
    fn test_global_set() -> anyhow::Result<()> {
        // (module
        //   (global $g (mut i32) (i32.const 10))
        //   (func (export "set") (param i32) local.get 0 global.set $g)
        //   (func (export "get") (result i32) global.get $g))
        let bytes = [
            b"\x00asm\x01\x00\x00\x00" as &[u8],
            // type section: 2 types — (i32)->() and ()->(i32)
            b"\x01\x09\x02\x60\x01\x7f\x00\x60\x00\x01\x7f",
            // function section: 2 funcs, type 0 and type 1
            b"\x03\x03\x02\x00\x01",
            // global section: 1 global, i32 mut, init=i32.const 10
            b"\x06\x06\x01\x7f\x01\x41\x0a\x0b",
            // export section: "set" -> func 0, "get" -> func 1
            b"\x07\x0d\x02\x03\x73\x65\x74\x00\x00\x03\x67\x65\x74\x00\x01",
            // code section: 2 funcs
            b"\x0a\x0d\x02\x06\x00\x20\x00\x24\x00\x0b\x04\x00\x23\x00\x0b",
        ]
        .concat();

        let mut runtime = Runtime::default();
        let mh = runtime.load_module(&bytes)?;

        // Initial value from init expr is 10
        let r = runtime.invoke(mh, "get", &[])?;
        assert!(
            matches!(r.as_slice(), [Value::I32(10)]),
            "expected 10 from init expr, got {r:?}",
        );

        // Set to 99, read back
        runtime.invoke(mh, "set", &[Value::I32(99)])?;
        let r = runtime.invoke(mh, "get", &[])?;
        assert!(
            matches!(r.as_slice(), [Value::I32(99)]),
            "expected 99 after set, got {r:?}",
        );

        // Set to -1, read back
        runtime.invoke(mh, "set", &[Value::I32(-1)])?;
        let r = runtime.invoke(mh, "get", &[])?;
        assert!(
            matches!(r.as_slice(), [Value::I32(-1)]),
            "expected -1 after set, got {r:?}",
        );

        Ok(())
    }
}
