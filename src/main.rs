extern crate framebuffer;
extern crate rscam;
extern crate rust_embed;
extern crate evdev;
extern crate rppal;

use framebuffer::{Framebuffer};
use std::fs::{File, read_dir, create_dir_all};
use std::io::{self, Read, Write, Seek, SeekFrom};
use rust_embed::RustEmbed;
use std::path::{Path};
use std::{thread, time};
use rppal::gpio::Gpio;
use rscam::{Camera, ResolutionInfo, CtrlData};
use rscam::{FLAG_DISABLED, CID_ROTATE};
use std::process::Command;

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
    return calibration_string.split(' ').map(|x| x.parse::<i32>().unwrap()).collect::<Vec<i32>>();
}

fn print_control_data(camera:&Camera) {
    // Camera supported formats
    
    for wformat in camera.formats() {
        let format = wformat.unwrap();
        println!("{:?}", format);

        let resolutions = camera.resolutions(&format.format).unwrap();

        if let ResolutionInfo::Discretes(d) = resolutions {
            for resol in &d {
                println!(
                    "  {}x{}  {:?}",
                    resol.0,
                    resol.1,
                    camera.intervals(&format.format, *resol).unwrap()
                );
            }
        } else {
            println!("  {:?}", resolutions);
        }
    }

    // Camera Controls
    for wctrl in camera.controls() {
        let ctrl = wctrl.unwrap();

        if let CtrlData::CtrlClass = ctrl.data {
            println!("\n[{}]\n", ctrl.name);
            continue;
        }

        print!("{:>32} ", ctrl.name);

        if ctrl.flags & FLAG_DISABLED != 0 {
            println!("(disabled)");
            continue;
        }

        match ctrl.data {
            CtrlData::Integer {
                value,
                default,
                minimum,
                maximum,
                step,
            } => println!(
                "(int)     min={} max={} step={} default={} value={}",
                minimum, maximum, step, default, value
            ),
            CtrlData::Boolean { value, default } => {
                println!("(bool)    default={} value={}", default, value)
            }
            CtrlData::Menu {
                value,
                default,
                ref items,
                ..
            } => {
                println!("(menu)    default={} value={}", default, value);
                for item in items {
                    println!("{:42} {}: {}", "", item.index, item.name);
                }
            }
            CtrlData::IntegerMenu {
                value,
                default,
                ref items,
                ..
            } => {
                println!("(intmenu) default={} value={}", default, value);
                for item in items {
                    println!(
                        "{:42} {}: {} ({:#x})",
                        "", item.index, item.value, item.value
                    );
                }
            }
            CtrlData::Bitmask {
                value,
                default,
                maximum,
            } => println!(
                "(bitmask) max={:x} default={:x} value={:x}",
                maximum, default, value
            ),
            CtrlData::Integer64 {
                value,
                default,
                minimum,
                maximum,
                step,
            } => println!(
                "(int64)   min={} max={} step={} default={} value={}",
                minimum, maximum, step, default, value
            ),
            CtrlData::String {
                ref value,
                minimum,
                maximum,
                step,
            } => println!(
                "(str)     min={} max={} step={} value={}",
                minimum, maximum, step, value
            ),
            CtrlData::Button => println!("(button)"),
            _ => {}
        }
    }
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

fn get_next_file_name () -> String {
    let path = "/home/pi/DCIM/";
    create_dir_all(path).expect("Could not create photo directory ~/DCIM");

    let entries = read_dir(path).expect("Could not create photo directory ~/DCIM")
        .filter_map(|v| v.ok())
        .filter_map(|e| e.path().into_os_string().into_string().ok())
        .collect::<Vec<String>>();

    let mut i:u32 = 1;
    loop {
        let filename: String = format!("{}photo{}.jpg", path, i);
        if entries.contains(&filename) {
            i += 1;
        } else {
            return filename;
        }
    }
}

fn main () -> io::Result<()> {

    let screensaver = Asset::get("cam.bmp").unwrap();
    let mut camera:Camera = rscam::new("/dev/video0").unwrap();
    let framebuffer: framebuffer::Framebuffer  = Framebuffer::new("/dev/fb1").unwrap();
    let mut file: std::fs::File = File::create("/dev/fb1").expect("Unable to open");
    let w = framebuffer.var_screen_info.xres;
    let h = framebuffer.var_screen_info.yres;
    let length:usize = (w * h) as usize;
    let mut buf:Vec<u16> = Vec::with_capacity(length);

    print_control_data(&camera);
    camera.set_control(CID_ROTATE, &0).unwrap();

    let calibration_data:Vec<i32> = get_calibration_data();
    println!("Calibration Data: {:?}", calibration_data);
    let mut touch_device = evdev::Device::open(&Path::new("/dev/input/touchscreen")).unwrap();
    let mut raw_touch:Coord = Coord { x:0, y:0 };
    let mut touch:Coord = Coord { x:0, y:0 };
    let mut is_touching:bool = false;

    // GPIO
    let gpio = Gpio::new().expect("Could not instantiate GPIO");

    let pin_27 = gpio.get(27).expect("Could not read pin 27");
    let pin_27input = pin_27.into_input_pullup();
    let mut pin_27_prev_val: u8 = 255;

    let pin_23 = gpio.get(23).expect("Could not read pin 23");
    let pin_23input = pin_23.into_input_pullup();
    let mut pin_23_prev_val: u8 = 255;

    let pin_22 = gpio.get(22).expect("Could not read pin 22");
    let pin_22input = pin_22.into_input_pullup();
    let mut pin_22_prev_val: u8 = 255;

    let pin_17 = gpio.get(17).expect("Could not read pin 17");
    let pin_17input = pin_17.into_input_pullup();
    let mut pin_17_prev_val: u8 = 255;

    println!("{}", touch_device);

    file.seek(SeekFrom::Start(0)).expect("Can't reset file pointer");
    file.write_all(&screensaver[138..]).expect("Can't write to framebuffer");

    let mut mode:u8 = 0; // 0 idle, 1 taking photo, 2, showing preview

    println!("Resolution {} {}", w, h);

    loop {
        thread::sleep(time::Duration::from_millis(48));

        let val27 = pin_27input.read() as u8;
        let val27_changing:bool = val27 != pin_27_prev_val;
        if val27_changing {
            println!("Pin State 27: {}", if val27 == 0 { "Pressed!" } else { "not pressed" });
            pin_27_prev_val = val27;
        }

        let val23 = pin_23input.read() as u8;
        if val23 != pin_23_prev_val {
            println!("Pin State 23: {}", if val23 == 0 { "Pressed!" } else { "not pressed" });
            pin_23_prev_val = val23;
        }

        let val22 = pin_22input.read() as u8;
        if val22 != pin_22_prev_val {
            println!("Pin State 22: {}", if val22 == 0 { "Pressed!" } else { "not pressed" });
            pin_22_prev_val = val22;
        }

        let val17 = pin_17input.read() as u8;
        if val17 != pin_17_prev_val {
            println!("Pin State 17: {}", if val17 == 0 { "Pressed!" } else { "not pressed" });
            pin_17_prev_val = val17;
        }

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
            println!("is touching? {}, Touch data {} {}", is_touching, touch.x, touch.y);
        }

        // Quit button
        if val17 == 0 {

            println!("Quitting");
            break;
        }

        match mode {

            // Waiting for input to start the camera
            0 => {
                if val27_changing && val27 == 0 {
                    camera.start(&rscam::Config {
                        interval: (1, 20),
                        resolution: (w, h),
                        format: b"RGB3",
                        ..Default::default()
                    }).unwrap();

                    println!("Mode 1");
                    mode = 1;
                    continue;
                }
            }

            // Showing the camera feed
            1 => {
                if val27_changing && val27 == 0 {
                    println!("Mode 2");
                    mode = 2;
                    continue;
                }


                let frame = camera.capture().unwrap();
            
                buf.clear();
                buf.extend(frame.chunks_exact(3).map(|px| {
                    ((px[0] as u16) >> 3) << 11 |
                    ((px[1] as u16) >> 2) << 5 |
                    ((px[2] as u16) >> 3)
                }));
            
                file.seek(SeekFrom::Start(0)).expect("Can't reset file pointer");
                let u8_slice: &[u8] = unsafe {
                    std::slice::from_raw_parts(buf.as_ptr().cast(), buf.len()*2)
                };
            
                file.write_all(&u8_slice).expect("Unable to write");
            }

            2 => {
                camera.stop().expect("Could not stop camera");
                camera = rscam::new("/dev/video0").unwrap();
                camera.start(&rscam::Config {
                    interval: (1, 10),
                    resolution: (4056, 3040),
                    format: b"JPEG",
                    ..Default::default()
                }).unwrap();
                let frame = camera.capture().unwrap();

                camera.stop().expect("Could not stop camera");
                camera = rscam::new("/dev/video0").unwrap();

                let filename = get_next_file_name();
                let mut fileout: std::fs::File = File::create(&filename).expect("Unable to open");
                fileout.write_all(&frame).expect("Unable to write");
                println!("Saved {}", &filename);

                let output = Command::new("sh")
                    .arg("-c")
                    .arg(format!("exiftool -orientation#=8 \"{}\" -m -P -overwrite_original", &filename))
                    .output()
                    .expect("failed to rotate image");
                println!("");
                io::stdout().write_all(&output.stdout).unwrap();

                file.seek(SeekFrom::Start(0)).expect("Can't reset file pointer");
                file.write_all(&screensaver[138..]).expect("Can't write to framebuffer");
                println!("Mode 0");
                mode = 0;
                continue;
            }

            _=>{}
        }
    }

    Ok(())
}
