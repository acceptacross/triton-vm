import functools
from enum import IntEnum, IntFlag, auto

class Instruction(IntEnum):
    Halt = 0
    Pop = auto()
    Push = auto()
    Divine = auto()
    Dup = auto()
    Swap = auto()
    Nop = auto()
    Skiz = auto()
    Call = auto()
    Return = auto()
    Recurse = auto()
    Assert = auto()
    ReadMem = auto()
    WriteMem = auto()
    Hash = auto()
    DivineSibling = auto()
    AssertVector = auto()
    Add = auto()
    Mul = auto()
    Invert = auto()
    Eq = auto()
    Split = auto()
    Lt = auto()
    And = auto()
    Xor = auto()
    Log2Floor = auto()
    Pow = auto()
    Div = auto()
    XxAdd = auto()
    XxMul = auto()
    XInvert = auto()
    XbMul = auto()
    ReadIo = auto()
    WriteIo = auto()

class InstructionBucket(IntFlag):
    HasArg = auto()
    ShrinkStack = auto()
    U32 = auto()

# ===

def in_bucket(instruction_bucket, instruction):
    if instruction_bucket == InstructionBucket.HasArg:
        return instruction in [Instruction.Push, Instruction.Dup, Instruction.Swap, Instruction.Call]
    if instruction_bucket == InstructionBucket.ShrinkStack:
        return instruction in [Instruction.Pop, Instruction.Skiz, Instruction.Assert, Instruction.WriteIo,
                               Instruction.Add, Instruction.Mul, Instruction.Eq, Instruction.XbMul]
    if instruction_bucket == InstructionBucket.U32:
        return instruction in [Instruction.Lt, Instruction.And, Instruction.Xor, Instruction.Log2Floor,
                               Instruction.Pow, Instruction.Div, Instruction.Split]
    return False

def flag_set(instruction):
    instruction_flags = [bucket for bucket in InstructionBucket if in_bucket(bucket, instruction)]
    return functools.reduce(lambda x, y: x | y, instruction_flags, 0)

def opcode(instruction):
    instruction_flag_set = flag_set(instruction)
    index_within_flag_set = 0
    for inst in Instruction:
        if inst < instruction and instruction_flag_set == flag_set(inst):
            index_within_flag_set += 1
    return index_within_flag_set * 2**len(InstructionBucket) + instruction_flag_set

def print_all_opcodes():
    for instruction in Instruction:
        opc = opcode(instruction)
        print(f"{opc:> 3} {opc:>07b} {str(instruction)}")

def print_max_opcode():
    max_opc = max([opcode(instruction) for instruction in Instruction])
    print(f"highest opcode: {max_opc}")
    print(f"#ibs:           {len(bin(max_opc)[2:])}")


# ===

def opcodes_are_unique_test():
    all_opcodes = [opcode(instruction) for instruction in Instruction]
    all_opcodes = sorted(all_opcodes)
    assert(list(set(all_opcodes)) == list(all_opcodes))

def tests():
    opcodes_are_unique_test()

# ===

tests()

print_all_opcodes()
print()
print_max_opcode()
