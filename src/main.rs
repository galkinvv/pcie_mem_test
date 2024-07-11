//cahce reread

use std::cmp;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{stdout, Write};
use std::time::Instant;

type MemSingleUnit = u32;
const UNIT_COUNT: usize = 8;

#[derive(PartialEq, Default, Clone, Copy)]
struct MemUnit([MemSingleUnit; UNIT_COUNT]);

const UNIT_ARRAY_BYTES: usize = std::mem::size_of::<MemUnit>();

impl std::fmt::Display for MemUnit {
    fn fmt(self: &MemUnit, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (i, x) in self.0.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                write!(f, "  ").expect("MemUnit spacing1");
                if i > 0 && i % 16 == 0 {
                    write!(f, "   ").expect("MemUnit spacing1");
                }
            }
            write!(f, "{:08x} ", x).expect("MemUnit Formatter");
        }
        Ok(())
    }
}

fn get_rotated_left_7_hex_digits(x: u32, shift_digits: u32) -> u32 {
    let hex_digit_bits = 4;
    let shift_bits = shift_digits * hex_digit_bits;
    let upper_digits_mask = 0xFFFFFFFFu32 << (32 - shift_bits - hex_digit_bits); //mask for shift_digits digits
    let x_with_bits_pre_shift =
        ((x & upper_digits_mask) << hex_digit_bits) | (x & !upper_digits_mask);
    x_with_bits_pre_shift.rotate_left(shift_bits)
}

fn index_to_single_value(phys_addr: usize) -> MemSingleUnit {
    let shift: u32 = ((1 + phys_addr) as u32) % 13 + 2;
    let rotated = get_rotated_left_7_hex_digits(phys_addr as u32, shift % 7);
    //add shift value as first hex deigit
    (rotated | (shift << (7 * 4))) as MemSingleUnit
}

fn index_to_value(i: usize) -> MemUnit {
    let mut result: MemUnit = Default::default();
    for x in 0..UNIT_COUNT {
        result.0[x] =
            index_to_single_value(i * UNIT_ARRAY_BYTES + x * std::mem::size_of::<MemSingleUnit>());
    }
    result
}

fn text_memunit_status(expected: MemUnit, read1: MemUnit, read2: MemUnit) -> &'static str {
    match (expected == read2, read1 == read2) {
        (false, false) => "UNSTABLE READ",
        (true, false) => "SINGLE",
        (false, true) => "READ SAME",
        (true, true) => panic!("called status for no error"),
    }
}

fn test_file(file_to_test: &File) -> Result<bool, Box<dyn Error>> {
    let mmaped_ro = unsafe { memmap2::Mmap::map(file_to_test) }.expect("mmap_ro_prepare_failed");
    let mut mmaped =
        unsafe { memmap2::MmapMut::map_mut(file_to_test) }.expect("mmap_mut_prepare_failed");
    let len_in_units = file_to_test.metadata().expect("metadata").len() as usize / UNIT_ARRAY_BYTES;
    let mem_ro_as_units_slices =
        unsafe { std::slice::from_raw_parts(mmaped_ro.as_ptr() as *const MemUnit, len_in_units) };
    let mem_as_units_slices = unsafe {
        std::slice::from_raw_parts_mut(mmaped.as_mut_ptr() as *mut MemUnit, len_in_units)
    };
    for (addr, value) in mem_as_units_slices.iter_mut().enumerate() {
        *value = index_to_value(addr);
    }
    let max_iteration = 3;
    for iteration in 0..max_iteration {
        let time_start = Instant::now();
        for (addr, value) in mem_as_units_slices.iter().enumerate() {
            let value_accessed = *value;
            if value_accessed != index_to_value(addr) {
                let reread = mem_ro_as_units_slices[addr];
                let show_range_pre = 65;
                let show_range_post = if cfg!(test) { 1 } else { 17 } * 1024 * 4;
                println!("FAIL: error found at iteration {}", iteration);
                for ok_addr in (cmp::max(show_range_pre, addr) - show_range_pre)..addr {
                    println!(
                        "{:#010x}: {}  OK",
                        ok_addr * UNIT_ARRAY_BYTES,
                        index_to_value(ok_addr)
                    );
                }
                println!(
                    "{:#010x}: {}",
                    addr * UNIT_ARRAY_BYTES,
                    index_to_value(addr)
                );
                println!("It{}  MEMBAR {}FAIL", iteration, value_accessed);
                println!(
                    "It{}  REREAD {}{}",
                    iteration,
                    reread,
                    text_memunit_status(index_to_value(addr), value_accessed, reread)
                );
                stdout().lock().flush()?;
                for (bad_addr, bad_value) in mem_as_units_slices
                    [addr + 1..cmp::min(addr + show_range_post, len_in_units)]
                    .iter()
                    .enumerate()
                {
                    let from_start_addr = addr + 1 + bad_addr;
                    let good_value = index_to_value(from_start_addr);
                    let bad_value_accessed = *bad_value;
                    if good_value != bad_value_accessed {
                        let bad_reread = mem_ro_as_units_slices[from_start_addr];
                        println!(
                            "{:#010x}: {}",
                            from_start_addr * UNIT_ARRAY_BYTES,
                            good_value
                        );
                        println!("It{}  MEMBAR {}FAIL", iteration, bad_value_accessed);
                        println!(
                            "It{}  REREAD {}{}",
                            iteration,
                            bad_reread,
                            text_memunit_status(good_value, bad_value_accessed, bad_reread)
                        );
                    } else {
                        println!(
                            "{:#010x}: {}  OK",
                            from_start_addr * UNIT_ARRAY_BYTES,
                            good_value
                        );
                    }
                }
                return Ok(false);
            }
        }
        if iteration == 0 {
            let elapsed = time_start.elapsed();
            print!(
                "First pass done without miscompares in {} milliseconds, ",
                elapsed.as_millis()
            );
            println!(
                "all {} iterations are expected to be done in {} seconds... ",
                max_iteration,
                (elapsed * max_iteration).as_secs()
            );
            stdout().lock().flush()?;
        }
    }
    println!("PASS: iterations {}", max_iteration);
    Ok(true)
}

fn main() -> Result<(), Box<dyn Error>> {
    print!("usage: boot with console=null and run as root passing path with a pcie memory bar mapping as a single argument. By https://github.com/galkinvv/pcie_mem_test  ");
    println!(
        "Typical example: {} /sys/bus/pci/devices/0000:01:00.0/resource1",
        env::args_os()
            .next()
            .unwrap_or("./pcie_mem_test".into())
            .to_string_lossy()
    );
    if env::args_os().len() != 2 {
        return Err("Expected exactly one command line agument".into());
    }
    let file_name = env::args_os()
        .nth(1)
        .expect("Single file argument is expected");
    println!(
        "Testing {0:?}, size {1}=0x{1:X} ... ",
        file_name,
        fs::metadata(&file_name)?.len()
    );
    stdout().lock().flush()?;
    let file = File::options()
        .read(true)
        .write(true)
        .open(file_name)
        .expect("Failed opening file for Read+Write");
    test_file(&file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time;
    use tempfile;

    #[test]
    fn test_index_to_value() {
        assert_eq!(index_to_single_value(0x00000000), 0x10000000);
        assert_eq!(index_to_single_value(0x00000001), 0x20000100);
        assert_eq!(index_to_single_value(0x0000000F), 0x20000F00);
        assert_eq!(index_to_single_value(0x000001F5), 0x5F500001);
        assert_eq!(index_to_single_value(0x000000F0), 0x300F0000);
        assert_eq!(index_to_single_value(0x00000100), 0x50000001);
        assert_eq!(index_to_single_value(0x00000F00), 0x5000000F);
        assert_eq!(index_to_single_value(0x00001000), 0x20100000);
        assert_eq!(index_to_single_value(0x00010000), 0x30000001);
        assert_eq!(index_to_single_value(0x00100000), 0x50001000);
        assert_eq!(index_to_single_value(0x01000000), 0x20000010);
        assert_eq!(index_to_single_value(0x0E000006), 0x0E000006);
    }

    #[test]
    fn test_on_modifying_file() -> Result<(), Box<dyn Error>> {
        let modifying = tempfile::tempfile().expect("failed creating temp file");
        let len: usize = 256 * 1024 * 1024;
        modifying.set_len(len as u64).expect("set_len");
        let mut mmap_concurrent =
            unsafe { memmap2::MmapMut::map_mut(&modifying) }.expect("mmap_concurrent_failed");
        thread::spawn(move || {
            thread::sleep(time::Duration::from_millis(
                25000 / std::mem::size_of::<MemUnit>() as u64,
            ));
            mmap_concurrent[len / 100] = 0x42;
        });
        assert!(!test_file(&modifying)?);
        Ok(())
    }
}
