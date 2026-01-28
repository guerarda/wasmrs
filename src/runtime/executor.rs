use std::{
    ops::{BitAnd, BitOr, BitXor},
    result,
};

use crate::{
    Error,
    binary::types::BlockType,
    instructions::Instruction,
    runtime::{Runtime, TrapError, instance::ModuleInstance, stack::Label, value::Value},
};

impl Runtime {
    fn unary_op_i32<F, R>(&mut self, unop: F)
    where
        F: FnOnce(i32) -> R,
        R: Into<i32>,
    {
        let lhs = self.value_stack.pop().unwrap();
        let res = match lhs {
            Value::I32(a) => unop(a).into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res));
    }

    fn binary_op_i32<F, R>(&mut self, binop: F)
    where
        F: FnOnce(i32, i32) -> R,
        R: Into<i32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a, b).into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res));
    }

    fn try_binary_op_i32<F, R>(&mut self, binop: F) -> result::Result<(), Error>
    where
        F: FnOnce(i32, i32) -> Option<R>,
        R: Into<i32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a, b).ok_or(TrapError::Unexpected)?.into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res));
        Ok(())
    }

    fn binary_op_u32<F, R>(&mut self, binop: F)
    where
        F: FnOnce(u32, u32) -> R,
        R: Into<u32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a as u32, b as u32).into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res as i32));
    }

    fn try_binary_op_u32<F, R>(&mut self, binop: F) -> Result<(), Error>
    where
        F: FnOnce(u32, u32) -> Option<R>,
        R: Into<u32>,
    {
        let rhs = self.value_stack.pop().unwrap();
        let lhs = self.value_stack.pop().unwrap();
        let res = match (lhs, rhs) {
            (Value::I32(a), Value::I32(b)) => binop(a as u32, b as u32)
                .ok_or(TrapError::Unexpected)?
                .into(),
            _ => unreachable!(),
        };
        self.value_stack.push(Value::I32(res as i32));
        Ok(())
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

    fn unwind_value_stack(value_stack: &mut Vec<Value>, sp: usize, arity: u32) {
        let results = {
            let idx = value_stack.len() - arity as usize;
            value_stack.split_off(idx)
        };
        value_stack.truncate(sp);
        value_stack.extend(results);
    }

    pub(super) fn execute(&mut self) -> Result<(), Error> {
        while let Some(frame) = self.call_stack.last_mut() {
            let func_inst = self.store.get_func(frame.funcaddr);
            let module_inst = self.module_registry.get_instance(func_inst.module);
            let instrs = &func_inst.func.body;

            frame.pc += 1;
            if let Some(inst) = instrs.get(frame.pc as usize) {
                match inst {
                    Instruction::Unreachable => {
                        return Err(TrapError::Unreachable.into());
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
                        frame.push_label(arity, end, self.value_stack.len());

                        let cond = self.value_stack.pop().unwrap();
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
                            Self::unwind_value_stack(&mut self.value_stack, sp, arity);
                            frame.pc = pc;
                        }
                        None => {
                            Self::unwind_value_stack(&mut self.value_stack, frame.sp, frame.arity);
                            self.call_stack.pop();
                        }
                    },
                    Instruction::Br(label_idx) => {
                        let label = frame.pop_nth_label(*label_idx);
                        Self::unwind_value_stack(&mut self.value_stack, label.sp, label.arity);
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
                                );
                                frame.pc = label.pc;
                            }
                            _ => unreachable!(),
                        };
                    }
                    Instruction::BrTable(_) => todo!(),
                    Instruction::Return => {
                        Self::unwind_value_stack(&mut self.value_stack, frame.sp, frame.arity);
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
                    Instruction::LocalSet(_) => todo!(),
                    Instruction::LocalTee(_) => todo!(),
                    Instruction::GlobalGet(_) => todo!(),
                    Instruction::GlobalSet(_) => todo!(),

                    Instruction::I32Load(_) => todo!(),
                    Instruction::I32Store(_) => todo!(),

                    Instruction::I32Const(v) => self.value_stack.push(Value::I32(*v)),
                    Instruction::I64Const(v) => self.value_stack.push(Value::I64(*v)),

                    // Comparison ops
                    Instruction::I32Eqz => self.unary_op_i32(|a| a == 0),
                    Instruction::I32Eq => self.binary_op_i32(|a, b| a == b),
                    Instruction::I32Ne => self.binary_op_i32(|a, b| a != b),
                    Instruction::I32LtS => self.binary_op_i32(|a, b| a < b),
                    Instruction::I32LtU => self.binary_op_u32(|a, b| a < b),
                    Instruction::I32GtS => self.binary_op_i32(|a, b| a > b),
                    Instruction::I32GtU => self.binary_op_u32(|a, b| a > b),
                    Instruction::I32LeS => self.binary_op_i32(|a, b| a <= b),
                    Instruction::I32LeU => self.binary_op_u32(|a, b| a <= b),
                    Instruction::I32GeS => self.binary_op_i32(|a, b| a >= b),
                    Instruction::I32GeU => self.binary_op_u32(|a, b| a >= b),

                    // Unary ops
                    Instruction::I32Clz => self.unary_op_i32(|a| a.leading_zeros() as i32),
                    Instruction::I32Ctz => self.unary_op_i32(|a| a.trailing_zeros() as i32),
                    Instruction::I32Popcnt => self.unary_op_i32(|a| a.count_ones() as i32),

                    // Arithmetic ops
                    Instruction::I32Add => self.binary_op_i32(|a, b| a.wrapping_add(b)),
                    Instruction::I32Sub => self.binary_op_i32(|a, b| a.wrapping_sub(b)),
                    Instruction::I32Mul => self.binary_op_i32(|a, b| a.wrapping_mul(b)),
                    Instruction::I32DivS => self.try_binary_op_i32(|a, b| a.checked_div(b))?,
                    Instruction::I32DivU => self.try_binary_op_u32(|a, b| a.checked_div(b))?,
                    Instruction::I32RemS => self.try_binary_op_i32(|a, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,

                    Instruction::I32RemU => self.try_binary_op_u32(|a, b| {
                        if b == 0 {
                            None
                        } else {
                            Some(a.wrapping_rem(b))
                        }
                    })?,
                    Instruction::I32And => self.binary_op_i32(BitAnd::bitand),
                    Instruction::I32Or => self.binary_op_i32(BitOr::bitor),
                    Instruction::I32Xor => self.binary_op_i32(BitXor::bitxor),
                    Instruction::I32Shl => self.binary_op_i32(|a, b| a.wrapping_shl(b as u32)),
                    Instruction::I32ShrS => self.binary_op_i32(|a, b| a.wrapping_shr(b as u32)),
                    Instruction::I32ShrU => self.binary_op_u32(|a, b| a.wrapping_shr(b)),
                    Instruction::I32Rotl => self.binary_op_i32(|a, b| a.rotate_left(b as u32)),
                    Instruction::I32Rotr => self.binary_op_i32(|a, b| a.rotate_right(b as u32)),

                    // Sign extension ops
                    Instruction::I32Extend8S => self.unary_op_i32(|a| a as i8),
                    Instruction::I32Extend16S => self.unary_op_i32(|a| a as i16),

                    // Ref
                    Instruction::RefNull(rt) => self.value_stack.push(Value::NullRef(*rt)),
                    Instruction::RefFunc(fi) => self.value_stack.push(Value::FuncRef(*fi)),
                }
            }
        }
        Ok(())
    }
}
