use std::cmp;
use std::env;
use std::fs::File;
use std::time::Instant;
use std::error::Error;
use std::io::{stdout, Write};
use std::fs;
use memmap2;

fn get_rotated_left_7_hex_digits(x: u32, shift_digits: u32) -> u32
{
    let hex_digit_bits = 4;
    let shift_bits = shift_digits * hex_digit_bits;
    let upper_digits_mask = 0xFFFFFFFFu32 << (32 - shift_bits - hex_digit_bits); //mask for shift_digits digits
    let x_with_bits_pre_shift = ((x & upper_digits_mask) << hex_digit_bits) | (x & !upper_digits_mask);
    x_with_bits_pre_shift.rotate_left(shift_bits)
}

fn index_to_value(i : usize) -> u32
{
    let shift : u32 = ((1 + i) as u32) % 7;
    let rotated = get_rotated_left_7_hex_digits(i as u32, shift);
    //add shift value as first hex deigit
    rotated | (shift << 7*4)
}

fn test_file(file_to_test: &File) -> Result<bool, Box<dyn Error>>
{
    let mut mmaped = unsafe { memmap2::MmapMut::map_mut(file_to_test) }.expect("mmap_prepare_failed");
    let len_in_u32 = (file_to_test.metadata().expect("metadata").len() / 4) as usize;
    let m_u32 = unsafe {
        std::slice::from_raw_parts_mut(mmaped.as_mut_ptr() as *mut u32, len_in_u32)
    };
    for (addr, value) in m_u32.iter_mut().enumerate()
    {
        *value = index_to_value(addr);
    }
    let max_iteration = 3;
    for iteration in 0..max_iteration
    {
        let time_start = Instant::now();
        for (addr, value) in m_u32.iter().enumerate()
        {
            if *value != index_to_value(addr)
            {
                let show_range_pre = 65;
                let show_range_post = 1024*1024*4;
                println!("FAIL: error found at iteration {}", iteration);
                for ok_addr in (cmp::max(show_range_pre, addr) - show_range_pre)..addr
                {
                    println!("{:#010x}: {:#010x}  OK", ok_addr, index_to_value(ok_addr));
                }
                println!("{:#010x}: {:#010x}    MEM:{:#010x}", addr, index_to_value(addr), value);
                for (bad_addr, bad_value) in m_u32[addr + 1 .. cmp::min(addr + show_range_post, len_in_u32) ].iter().enumerate()
                {
                    let from_start_addr = addr + 1 + bad_addr;
                    let good_value = index_to_value(from_start_addr);
                    let bad_value_accessed = *bad_value;
                    if good_value != bad_value_accessed
                    {
                        println!("{:#010x}: {:#010x}    MEM:{:#010x}", from_start_addr, good_value, bad_value_accessed);
                    }
                    else
                    {
                        println!("{:#010x}: {:#010x}  OK", from_start_addr, good_value);
                    }
                }
                return Ok(false);
            }
        }
        if iteration == 0
        {
            let elapsed = time_start.elapsed();
            print!("First pass done without miscompares in {} milliseconds, ", elapsed.as_millis());
            print!("all {} iterations are expected to be done in {} seconds... ", max_iteration, (elapsed * max_iteration).as_secs());
            stdout().lock().flush()?;
        }
    }
    println!("PASS: iterations {}", max_iteration);
    Ok(true)
}

fn main() -> Result<(), Box<dyn Error>> {
    print!("usage: boot with console=null and run as root passing path with a pcie memory bar mapping as a single argument. By https://github.com/galkinvv/pcie_mem_test");
    println!("Typical example: {} /sys/bus/pci/devices/0000:01:00.0/resource1", env::args_os().next().unwrap_or("./pcie_mem_test".into()).to_string_lossy());
    if env::args_os().len() != 2
    {
        return Err("Expected exactly one command line agument".into());
    }
    let file_name = env::args_os().nth(1).expect("Single file argument is expected");
    print!("Testing {0:?}, size {1}=0x{1:X} ... ", file_name, fs::metadata(&file_name)?.len());
    stdout().lock().flush()?;
    let file = File::options().read(true).write(true).open(file_name).expect("Failed opening file for Read+Write");
    test_file(&file)?;
    Ok(())
}

#[cfg(test)]
mod tests
{
    use std::thread;
    use tempfile;
    use std::time;
    use super::*;

    #[test]
    fn test_index_to_value()
    {
        assert_eq!(index_to_value(0x00000000), 0x10000000);
        assert_eq!(index_to_value(0x00000001), 0x20000100);
        assert_eq!(index_to_value(0x0000000F), 0x20000F00);
        assert_eq!(index_to_value(0x000001F5), 0x5F500001);
        assert_eq!(index_to_value(0x000000F0), 0x300F0000);
        assert_eq!(index_to_value(0x00000100), 0x50000001);
        assert_eq!(index_to_value(0x00000F00), 0x5000000F);
        assert_eq!(index_to_value(0x00001000), 0x20100000);
        assert_eq!(index_to_value(0x00010000), 0x30000001);
        assert_eq!(index_to_value(0x00100000), 0x50001000);
        assert_eq!(index_to_value(0x01000000), 0x20000010);
        assert_eq!(index_to_value(0x0E000006), 0x0E000006);
    }

    #[test]
    fn test_on_modifying_file() -> Result<(), Box<dyn Error>>
    {
        let modifying = tempfile::tempfile().expect("failed creating temp file");
        let len: usize = 256*1024*1024;
        modifying.set_len(len as u64).expect("set_len");
        let mut mmap_concurrent = unsafe { memmap2::MmapMut::map_mut(&modifying)  }.expect("mmap_concurrent_failed");
        thread::spawn(move || {
                thread::sleep(time::Duration::from_millis(500));
                mmap_concurrent[len/2] = 42;
            });
        assert!(!test_file(&modifying)?);
        Ok(())
    }
}
