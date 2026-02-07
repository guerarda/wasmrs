use std::ops::{BitAnd, BitOr, BitXor};

use crate::{
    binary::types::BlockType,
    instructions::Instruction,
    runtime::{Runtime, RuntimeError, instance::ModuleInstance, stack::Label, value::Value},
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

    fn block_arity(bt: &BlockType, module_inst: &ModuleInstance) -> u32 {
        match bt {
            BlockType::Empty => 0,
            BlockType::Value(_) => 1,
            BlockType::Index(idx) => {
                let ftype = &module_inst.types[*idx as usize];
                ftype.results.len() as u32
            }
        }
    }

    fn unwind_value_stack(
        value_stack: &mut Vec<Value>,
        sp: usize,
        arity: u32,
    ) -> Result<(), RuntimeError> {
        let arity = arity as usize;

        if value_stack.len() < arity {
            return Err(RuntimeError::internal());
        }

        let results_idx = value_stack.len() - arity;
        if results_idx < sp {
            return Err(RuntimeError::internal());
        }
        // Rotate the results down to sp, then truncate
        value_stack[sp..].rotate_left(results_idx - sp);
        value_stack.truncate(sp + arity);

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
                        let arity = Self::block_arity(bt, module_inst);
                        let end = Self::find_block_end(instrs, frame.pc);
                        frame.push_label(arity, end, self.value_stack.len())
                    }
                    Instruction::Loop(bt) => {
                        let arity = Self::block_arity(bt, module_inst);
                        frame.push_label(arity, frame.pc, self.value_stack.len())
                    }
                    Instruction::If(bt) => {
                        let arity = Self::block_arity(bt, module_inst);
                        let (end, else_) = Self::find_if_else_end(instrs, frame.pc);

                        let cond = self.value_stack.pop().ok_or(RuntimeError::trap())?;
                        frame.push_label(arity, end, self.value_stack.len());

                        match cond {
                            Value::I32(0) => {
                                frame.pc = else_.unwrap_or(end);
                            }
                            Value::I32(_) => continue,
                            _ => unreachable!(),
                        };
                    }
                    Instruction::Else => {
                        frame.pc = frame.current_label().pc;
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
                        let label = frame.pop_nth_label(*label_idx);
                        Self::unwind_value_stack(&mut self.value_stack, label.sp, label.arity)?;
                        frame.pc = label.pc;
                    }
                    Instruction::BrIf(label_idx) => {
                        let cond = self.value_stack.pop().unwrap();
                        match cond {
                            Value::I32(0) => continue,

                            Value::I32(_) => {
                                let label = frame.pop_nth_label(*label_idx);
                                Self::unwind_value_stack(
                                    &mut self.value_stack,
                                    label.sp,
                                    label.arity,
                                )?;
                                frame.pc = label.pc;
                            }
                            _ => unreachable!(),
                        };
                    }
                    Instruction::BrTable(_) => todo!(),
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
                        let val1 = self.value_stack.pop().unwrap();
                        let val2 = self.value_stack.pop().unwrap();

                        match cond {
                            Value::I32(0) => self.value_stack.push(val2),
                            Value::I32(_) => self.value_stack.push(val1),
                            _ => unreachable!(),
                        }
                    }
                    Instruction::SelectT(_vt) => {
                        let cond = self.value_stack.pop().unwrap();
                        let val1 = self.value_stack.pop().unwrap();
                        let val2 = self.value_stack.pop().unwrap();

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
                    Instruction::MemorySize(_) => todo!(),
                    Instruction::MemoryGrow(_) => todo!(),

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
