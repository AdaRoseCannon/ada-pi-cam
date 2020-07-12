extern crate framebuffer;
extern crate rscam;

use framebuffer::{Framebuffer};
use std::fs::File;
use std::io::Write;

fn main () {
    let mut framebuffer = Framebuffer::new("/dev/fb1").unwrap();

    let w = framebuffer.var_screen_info.xres;
    let h = framebuffer.var_screen_info.yres;
    let length = w * h;
    println!("Resolution {} {}", w, h);

    let mut camera = rscam::new("/dev/video0").unwrap();

    for wformat in camera.formats() {
        let format = wformat.unwrap();
        println!("{:?}", format);
        println!("  {:?}", camera.resolutions(&format.format).unwrap());
    }

    camera.start(&rscam::Config {
        interval: (1, 10),      // 10 fps.
        resolution: (w, h),
        format: b"RGB3",
        ..Default::default()
    }).unwrap();

    let mut u8_buffer: Vec<u8> = Vec::with_capacity((length * 2) as usize);

    for i in 0..300 {
        u8_buffer.clear();
        let frame = camera.capture().unwrap();
        let mut file = File::create("/dev/fb1").expect("Unable to open");

        for x in 0..length {
            let red =   frame[(x*3 + 0) as usize];
            let green = frame[(x*3 + 1) as usize];
            let blue =  frame[(x*3 + 2) as usize];
            u8_buffer.push((green >> 2 << 5) + (blue >> 3));
            u8_buffer.push((red >> 3 << 3) + (green >> 5));
        }
        file.write_all(&u8_buffer).expect("Unable to write");
    }
}