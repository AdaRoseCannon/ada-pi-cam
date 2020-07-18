extern crate framebuffer;
extern crate rscam;

use framebuffer::{Framebuffer};
use std::fs::File;
use std::io::{Write, Seek, SeekFrom};

fn main () {

    let mut camera = rscam::new("/dev/video0").unwrap();
    let framebuffer = Framebuffer::new("/dev/fb1").unwrap();
    let mut file = File::create("/dev/fb1").expect("Unable to open");

    let w = framebuffer.var_screen_info.xres;
    let h = framebuffer.var_screen_info.yres;
    let length:usize = (w * h) as usize;
    println!("Resolution {} {}", w, h);

    for wformat in camera.formats() {
        let format = wformat.unwrap();
        println!("{:?}", format);
        println!("  {:?}", camera.resolutions(&format.format).unwrap());
    }

    camera.start(&rscam::Config {
        interval: (1, 30),
        resolution: (w, h),
        format: b"RGB3",
        ..Default::default()
    }).unwrap();

    let mut buf:Vec<u16> = Vec::with_capacity(length);
    loop {
        let frame = camera.capture().unwrap();
        file.seek(SeekFrom::Start(0)).expect("can't reset file pointer");

        buf.clear();
        buf.extend(frame.chunks_exact(3).map(|px| {
            ((px[0] as u16) >> 3) << 11 |
            ((px[1] as u16) >> 2) << 5 |
            ((px[2] as u16) >> 3)
        }));
    
        let u8_slice: &[u8] = unsafe {
            std::slice::from_raw_parts(buf.as_ptr().cast(), length*2)
        };

        file.write_all(&u8_slice).expect("Unable to write");
        // framebuffer.write_frame(&u8_slice);
    }
}