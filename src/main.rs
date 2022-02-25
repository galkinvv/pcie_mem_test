use std::cmp;
use std::env;
use std::fs::File;
use std::time::Instant;
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

fn test_file(file_to_test: &File) -> bool
{
    let mut mmaped = unsafe { memmap2::MmapMut::map_mut(file_to_test) }.expect("mmap_prepare_failed");
    let len_in_u32 = (file_to_test.metadata().expect("metadata").len() / 4) as usize;
    println!("u32 units: {}", len_in_u32);
    let m_u32 = unsafe {
        std::slice::from_raw_parts_mut(mmaped.as_mut_ptr() as *mut u32, len_in_u32)
    };
    for (addr, value) in m_u32.iter_mut().enumerate()
    {
        *value = index_to_value(addr);
    }
    let max_iteration = 1000;
    for iteration in 0..max_iteration
    {
        let time_start = Instant::now();
        for (addr, value) in m_u32.iter().enumerate()
        {
            if *value != index_to_value(addr)
            {
                let show_range_pre = 32;
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
                return false;
            }
        }
        if iteration == 0
        {
            let elapsed = time_start.elapsed();
            println!("First pass done without miscompares in {} milliseconds", elapsed.as_millis());
            println!("All {} iterations are expected to be done in {} seconds", max_iteration, (elapsed * max_iteration).as_secs());
        }
    }
    println!("PASS: iterations {}", max_iteration);
    true
}

fn main() {
    let file_name = env::args_os().nth(1).expect("Single file argument is expected");
    println!("Testing {:?}", file_name);
    let file = File::open(file_name).expect("Failed opening file");
    test_file(&file);
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
    fn test_on_modifying_file()
    {
        let modifying = tempfile::tempfile().expect("failed creating temp file");
        let len: usize = 256*1024*1024;
        modifying.set_len(len as u64).expect("set_len");
        let mut mmap_concurrent = unsafe { memmap2::MmapMut::map_mut(&modifying)  }.expect("mmap_concurrent_failed");
        thread::spawn(move || {
                thread::sleep(time::Duration::from_millis(2000));
                mmap_concurrent[len/2] = 42;
            });
        assert!(!test_file(&modifying));
    }
}
