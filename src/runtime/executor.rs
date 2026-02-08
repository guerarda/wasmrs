use std::ops::{BitAnd, BitOr, BitXor};

use crate::{
    binary::types::BlockType,
    instructions::Instruction,
    runtime::{
        Runtime, RuntimeError,
        instance::ModuleInstance,
        stack::{Frame, Label},
        value::Value,
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

macro_rules! try_binary_op {
    ($self:expr, $variant:ident, $op:expr) => {{
        let rhs = $self.value_stack.pop().unwrap();
        let lhs = $self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::$variant(a), Value::$variant(b)) => $op(a, b).ok_or(RuntimeError::trap())?,
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
                $op(a as $ty, b as $ty).ok_or(RuntimeError::trap())?
            }
            _ => unreachable!(),
        };
        $self.value_stack.push(Value::$variant(res as _));
        Ok::<(), RuntimeError>(())
    }};
}

impl Runtime {
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
    ) -> Result<(), RuntimeError> {
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
        stack: &mut Vec<Value>,
        frame: &mut Frame,
        label_idx: &u32,
    ) -> Result<(), RuntimeError> {
        let label = frame.pop_nth_label(*label_idx);
        Self::unwind_value_stack(stack, label.sp, label.arity)?;
        frame.pc = label.pc;
        Ok(())
    }

    pub(super) fn execute(&mut self) -> Result<(), RuntimeError> {
        while let Some(frame) = self.call_stack.last_mut() {
            let func_inst = self.store.get_func(frame.funcaddr);
            let module_inst = self.module_registry.get_instance(func_inst.module);
            let instrs = &func_inst.func.body;

            frame.pc += 1;
            if let Some(inst) = instrs.get(frame.pc as usize) {
                match inst {
                    Instruction::Unreachable => {
                        return Err(RuntimeError::trap());
                    }
                    Instruction::Nop => continue,
                    Instruction::Block(bt) => {
                        let (n_params, n_results) = Self::block_arity(bt, module_inst);
                        let end = Self::find_block_end(instrs, frame.pc);
                        frame.push_label(n_results, end, self.value_stack.len() - n_params as usize)
                    }
                    Instruction::Loop(bt) => {
                        let (n_params, _) = Self::block_arity(bt, module_inst);
                        frame.push_label(
                            n_params,
                            frame.pc,
                            self.value_stack.len() - n_params as usize,
                        )
                    }
                    Instruction::If(bt) => {
                        let (n_params, n_results) = Self::block_arity(bt, module_inst);
                        let (end, else_) = Self::find_if_else_end(instrs, frame.pc);

                        let cond = self.value_stack.pop().ok_or(RuntimeError::trap())?;
                        frame.push_label(
                            n_results,
                            end,
                            self.value_stack.len() - n_params as usize,
                        );

                        match cond {
                            Value::I32(0) => {
                                // Next instruction is one past else or end
                                frame.pc = else_.unwrap_or(end - 1);
                            }
                            Value::I32(_) => continue,
                            _ => unreachable!(),
                        };
                    }
                    Instruction::Else => {
                        frame.pc = frame.current_label().pc - 1;
                    }
                    Instruction::End => match frame.pop_label() {
                        Some(Label { arity, pc, sp }) => {
                            Self::unwind_value_stack(&mut self.value_stack, sp, arity)?;
                            frame.pc = pc;
                        }
                        None => {
                            Self::unwind_value_stack(&mut self.value_stack, frame.sp, frame.arity)?;
                            self.call_stack.pop();
                        }
                    },
                    Instruction::Br(label_idx) => {
                        Self::branch(&mut self.value_stack, frame, label_idx)?;
                    }
                    Instruction::BrIf(label_idx) => {
                        let cond = self
                            .value_stack
                            .pop()
                            .and_then(Value::as_bool)
                            .ok_or(RuntimeError::internal("br_if, invalid cond type"))?;

                        if cond {
                            Self::branch(&mut self.value_stack, frame, label_idx)?;
                        }
                    }
                    Instruction::BrTable(br_idx) => {
                        let i = self
                            .value_stack
                            .pop()
                            .and_then(Value::as_i32)
                            .ok_or(RuntimeError::internal("br_table, invalid index type"))?
                            as usize;

                        let label_idx = br_idx.labels.get(i).unwrap_or(&br_idx.default);
                        Self::branch(&mut self.value_stack, frame, label_idx)?;
                    }
                    Instruction::Return => {
                        Self::unwind_value_stack(&mut self.value_stack, frame.sp, frame.arity)?;
                        self.call_stack.pop();
                    }
                    Instruction::Call(idx) => {
                        let mi = self.module_registry.get_instance(func_inst.module);
                        let funcaddr = mi.funcaddrs[*idx as usize];
                        self.call(funcaddr);
                    }
                    Instruction::CallIndirect(_) => todo!(),
                    Instruction::Drop => {
                        self.value_stack.pop();
                    }
                    Instruction::Select => {
                        let cond = self.value_stack.pop().unwrap();
                        let val2 = self.value_stack.pop().unwrap();
                        let val1 = self.value_stack.pop().unwrap();

                        match cond {
                            Value::I32(0) => self.value_stack.push(val2),
                            Value::I32(_) => self.value_stack.push(val1),
                            _ => unreachable!(),
                        }
                    }
                    Instruction::SelectT(_vt) => {
                        let cond = self.value_stack.pop().unwrap();
                        let val2 = self.value_stack.pop().unwrap();
                        let val1 = self.value_stack.pop().unwrap();

                        match cond {
                            Value::I32(0) => self.value_stack.push(val2),
                            Value::I32(_) => self.value_stack.push(val1),
                            _ => unreachable!(),
                        }
                    }
                    Instruction::LocalGet(idx) => {
                        let v = frame.locals[*idx as usize];
                        self.value_stack.push(v)
                    }
                    Instruction::LocalSet(idx) => {
                        let v = self.value_stack.pop().unwrap();
                        frame.locals[*idx as usize] = v;
                    }
                    Instruction::LocalTee(_) => todo!(),
                    Instruction::GlobalGet(_) => todo!(),
                    Instruction::GlobalSet(_) => todo!(),

                    Instruction::I32Load(_) => todo!(),
                    Instruction::I32Store(_) => todo!(),
                    Instruction::MemorySize(idx) => {
                        let sz = Self::memory_size(&self.memories, *idx)?;
                        self.value_stack.push(Value::I32(sz as i32));
                    }
                    Instruction::MemoryGrow(idx) => {
                        let inc =
                            self.value_stack.pop().and_then(Value::as_i32).ok_or(
                                RuntimeError::internal("memory.grow, invalid argument type"),
                            )?;
                        let res = Self::memory_grow(&mut self.memories, *idx, inc as u32)?
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

                    // Conversion ops
                    Instruction::I64ExtendI32S => {
                        let val = self.value_stack.pop().unwrap();
                        let result = match val {
                            Value::I32(a) => Value::I64(i64::from(a)),
                            _ => unreachable!(),
                        };
                        self.value_stack.push(result);
                    }
                    Instruction::I64ExtendI32U => {
                        let val = self.value_stack.pop().unwrap();
                        let result = match val {
                            // Musn't do sign extension, so cast as u32 first
                            Value::I32(a) => Value::I64(a as u32 as i64),
                            _ => unreachable!(),
                        };
                        self.value_stack.push(result);
                    }

                    // Sign extension ops
                    Instruction::I32Extend8S => unary_op!(self, I32, |a| a as i8),
                    Instruction::I32Extend16S => unary_op!(self, I32, |a| a as i16),
                    Instruction::I64Extend8S => unary_op!(self, I64, |a| a as i8),
                    Instruction::I64Extend16S => unary_op!(self, I64, |a| a as i16),
                    Instruction::I64Extend32S => unary_op!(self, I64, |a| a as i32),

                    // Ref
                    Instruction::RefNull(rt) => self.value_stack.push(Value::NullRef(*rt)),
                    Instruction::RefFunc(fi) => self.value_stack.push(Value::FuncRef(*fi)),
                    Instruction::I32TruncSatF32S => todo!(),
                    Instruction::I32TruncSatF32U => todo!(),
                    Instruction::I64TruncSatF64S => todo!(),
                    Instruction::I64TruncSatF64U => todo!(),
                }
            }
        }
        Ok(())
    }
}

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
    #[ignore]
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
}
