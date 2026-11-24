// SPDX-License-Identifier: MIT
// Plays the four effects, then 3 s of the music. usage: audio_demo
use std::{thread::sleep, time::Duration};
use zerotag_core::audio::{play, start_music, stop_music, Sound};

fn main() {
    for (name, s, wait) in [("Blip", Sound::Blip, 400), ("Start", Sound::Start, 700), ("Error", Sound::Error, 800), ("Success", Sound::Success, 1300)] {
        println!("{name}");
        play(s);
        sleep(Duration::from_millis(wait));
    }
    println!("music");
    start_music();
    sleep(Duration::from_secs(3));
    stop_music();
    sleep(Duration::from_millis(300));
    println!("done");
}
