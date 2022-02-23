use std::fs::File;
use std::env;
use std::thread;
use std::time;
use tempfile;
use memmap2;

/*

let mut contents = Vec::new();
file.read_to_end(&mut contents)?;

let mmap = unsafe { Mmap::map(&file)?  };

assert_eq!(&contents[..], &mmap[..]);
*/
//use futures::executor::block_on;
fn index_to_value(i : usize) -> u32
{
    i as u32
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
    for _count in 0..100
    {
        for (addr, value) in m_u32.iter().enumerate()
        {
            if *value != index_to_value(addr)
            {
                return false;
            }
        }
    }
    true
}
fn main() {
    let file_name = env::args_os().nth(1).expect("Single file argument is expected");
    println!("Testing {:?}", file_name);
    let file = File::open(file_name).expect("Failed opening file");
    test_file(&file);
}

#[test]
fn test_on_modifying_file()
{
    let modifying = tempfile::tempfile().expect("failed creating temp file");
    let len: usize = 256*1024*1024;
    modifying.set_len(len as u64).expect("set_len");
    let mut mmap_concurrent = unsafe { memmap2::MmapMut::map_mut(&modifying)  }.expect("mmap_concurrent_failed");
    thread::spawn(move || {
            thread::sleep(time::Duration::from_millis(200));
            mmap_concurrent[len/2] = 42;
        });
    assert!(!test_file(&modifying));
}
