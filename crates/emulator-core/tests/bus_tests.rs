use emulator_core::{ArrayMemory, Endianness, MemoryBus, MemoryError, SliceMemory};

#[cfg(feature = "alloc")]
use emulator_core::DynamicMemory;

#[test]
fn test_array_memory_no_std() {
    let mut mem = ArrayMemory::<1024>::new();
    assert_eq!(mem.len(), 1024);

    // Byte read / write
    mem.write_u8(0, 0x42).unwrap();
    assert_eq!(mem.read_u8(0).unwrap(), 0x42);

    // Out of bounds
    assert_eq!(mem.read_u8(1024), Err(MemoryError::OutOfBounds(1024)));
    assert_eq!(mem.write_u8(1024, 0x99), Err(MemoryError::OutOfBounds(1024)));

    // Multi-byte operations Little Endian
    mem.write_u16(10, 0x1234, Endianness::LittleEndian).unwrap();
    assert_eq!(mem.read_u16(10, Endianness::LittleEndian).unwrap(), 0x1234);
    assert_eq!(mem.read_u8(10).unwrap(), 0x34);
    assert_eq!(mem.read_u8(11).unwrap(), 0x12);

    // Multi-byte operations Big Endian
    mem.write_u32(20, 0x11223344, Endianness::BigEndian).unwrap();
    assert_eq!(mem.read_u32(20, Endianness::BigEndian).unwrap(), 0x11223344);
    assert_eq!(mem.read_u8(20).unwrap(), 0x11);
    assert_eq!(mem.read_u8(21).unwrap(), 0x22);
    assert_eq!(mem.read_u8(22).unwrap(), 0x33);
    assert_eq!(mem.read_u8(23).unwrap(), 0x44);

    // 64-bit
    mem.write_u64(30, 0x0102030405060708, Endianness::LittleEndian).unwrap();
    assert_eq!(
        mem.read_u64(30, Endianness::LittleEndian).unwrap(),
        0x0102030405060708
    );
}

#[test]
fn test_slice_memory() {
    let mut buf = [0u8; 256];
    let mut mem = SliceMemory::new(&mut buf);

    mem.write_u16(0, 0xAABB, Endianness::BigEndian).unwrap();
    assert_eq!(mem.read_u16(0, Endianness::BigEndian).unwrap(), 0xAABB);
}

#[test]
#[cfg(feature = "alloc")]
fn test_dynamic_memory() {
    let mut mem = DynamicMemory::new(512);
    assert_eq!(mem.len(), 512);

    mem.write_u32(100, 0xCAFEBABE, Endianness::LittleEndian).unwrap();
    assert_eq!(
        mem.read_u32(100, Endianness::LittleEndian).unwrap(),
        0xCAFEBABE
    );
}
