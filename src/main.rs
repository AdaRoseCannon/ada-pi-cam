extern crate framebuffer;
extern crate rscam;
extern crate rppal;
extern crate rust_embed;

use framebuffer::{Framebuffer};
use std::fs::File;
use std::io::{Write, Seek, SeekFrom};
use rppal::gpio::Gpio;
use rust_embed::RustEmbed;


#[derive(RustEmbed)]
#[folder = "assets/"]
struct Asset;

fn main () {

    let screensaver = Asset::get("cam.bmp").unwrap();
    let mut camera: rscam::Camera = rscam::new("/dev/video0").unwrap();
    let framebuffer: framebuffer::Framebuffer  = Framebuffer::new("/dev/fb1").unwrap();
    let mut file: std::fs::File = File::create("/dev/fb1").expect("Unable to open");
    let w = framebuffer.var_screen_info.xres;
    let h = framebuffer.var_screen_info.yres;
    let length:usize = (w * h) as usize;
    let mut buf:Vec<u16> = Vec::with_capacity(length);

    file.write_all(&screensaver[138..]).expect("Can't write to framebuffer");

    let mut mode:u8 = 0; // 0 idle, 1 taking photo, 2, showing preview

    println!("Resolution {} {}", w, h);

    let gpio = Gpio::new().expect("Could not open GPIO");
    let gpio17 = gpio.get(27).expect("Could not open GPIO 17");
    let gpio17input = gpio17.into_input();
    let mut gpio17input_prev:bool = false;

    loop {
        match mode {
            0 => {
                let val = gpio17input.is_high();
                if val != gpio17input_prev {
                    println!("GPIO 17 is high? {}", val);
                    match val {
                        false => {}
                        true => {
                            mode = 1;
                            camera.start(&rscam::Config {
                                interval: (1, 30),
                                resolution: (w, h),
                                format: b"RGB3",
                                ..Default::default()
                            }).unwrap();
                        }
                    }
                }
                gpio17input_prev = val;
            }   
            1 => {

                file.seek(SeekFrom::Start(0)).expect("Can't reset file pointer");

                let frame = camera.capture().unwrap();
            
                buf.clear();
                buf.extend(frame.chunks_exact(3).map(|px| {
                    ((px[0] as u16) >> 3) << 11 |
                    ((px[1] as u16) >> 2) << 5 |
                    ((px[2] as u16) >> 3)
                }));
            
                let u8_slice: &[u8] = unsafe {
                    std::slice::from_raw_parts(buf.as_ptr().cast(), buf.len()*2)
                };
            
                file.write_all(&u8_slice).expect("Unable to write");
            }
            _=>{}
        }
    }
}