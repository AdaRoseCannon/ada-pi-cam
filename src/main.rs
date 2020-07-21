extern crate framebuffer;
extern crate rscam;
extern crate rust_embed;
extern crate evdev;

use framebuffer::{Framebuffer};
use std::fs::File;
use std::io::{Read, Write, Seek, SeekFrom};
use rust_embed::RustEmbed;
use std::path::Path;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Asset;

// Calibration data stored in /etc/pointercal
// xscale xymix xoffset yxmix yscale yoffset scaler
// Xs = (Xt*xscale + Yt*xymix + xoffset)/scaler
// Ys = (Xt*yxmix + Yt*yscale + yoffset)/scaler
// Source: https://www.qtcentre.org/threads/3793-etc-pointercal-problem
fn get_calibration_data() -> Vec<i32> {
    let mut calibration_file = std::fs::File::open("/etc/pointercal").unwrap();
    let mut calibration_string = String::new();
    calibration_file.read_to_string(&mut calibration_string).unwrap();
    let mut out_data:Vec<i32> = calibration_string.rsplit(' ').map(|x| x.parse::<i32>().unwrap()).collect();
    out_data.reverse();
    return out_data;
}

fn convert_touch_coords(calibration_data:&Vec<i32>, incoord:&Coord, out:&mut Coord) {
    let xscale = calibration_data[0]; 
    let xymix = calibration_data[1]; 
    let xoffset = calibration_data[2]; 
    let yxmix = calibration_data[3]; 
    let yscale = calibration_data[4]; 
    let yoffset = calibration_data[5];
    let scaler = calibration_data[6];

    out.x = incoord.x*(xscale/scaler) + incoord.y*xymix/scaler + xoffset/scaler; 
    out.y = incoord.y*(yscale/scaler) + incoord.x*yxmix/scaler + yoffset/scaler; 
}

struct Coord {
    x: i32,
    y: i32
}

fn main () {

    let screensaver = Asset::get("cam.bmp").unwrap();
    let mut camera: rscam::Camera = rscam::new("/dev/video0").unwrap();
    let framebuffer: framebuffer::Framebuffer  = Framebuffer::new("/dev/fb1").unwrap();
    let mut file: std::fs::File = File::create("/dev/fb1").expect("Unable to open");
    let w = framebuffer.var_screen_info.xres;
    let h = framebuffer.var_screen_info.yres;
    let length:usize = (w * h) as usize;
    let mut buf:Vec<u16> = Vec::with_capacity(length);

    let calibration_data:Vec<i32> = get_calibration_data();
    println!("Calibration Data: {:?}", calibration_data);
    let mut touch_device = evdev::Device::open(&Path::new("/dev/input/touchscreen")).unwrap();
    let mut raw_touch:Coord = Coord { x:0, y:0 };
    let mut touch:Coord = Coord { x:0, y:0 };
    let mut is_touching:bool = false;

    println!("{}", touch_device);

    file.write_all(&screensaver[138..]).expect("Can't write to framebuffer");

    let mut mode:u8 = 0; // 0 idle, 1 taking photo, 2, showing preview

    println!("Resolution {} {}", w, h);

    loop {
        for ev in touch_device.events().unwrap() {
            if ev._type == 1 && ev.code == 330 {
                is_touching=ev.value==1;
            }
            if ev._type == 3 && ev.code == 0 {
                raw_touch.x=ev.value;
            }
            if ev._type == 3 && ev.code == 1 {
                raw_touch.y=ev.value;
            }
        }
        if is_touching {
            convert_touch_coords(&calibration_data, &raw_touch, &mut touch);
            // println!("is touching? {}, Touch data {} {}", is_touching, touch.x, touch.y);
        }

        match mode {
            0 => {
                if is_touching {
                    camera.start(&rscam::Config {
                        interval: (1, 20),
                        resolution: (w, h),
                        format: b"RGB3",
                        ..Default::default()
                    }).unwrap();
                    mode = 1;
                }
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