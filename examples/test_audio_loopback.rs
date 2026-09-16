use audiopus::{coder::Encoder, Application, Bitrate, Channels, SampleRate};

fn main() {
    let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio)
        .expect("Init encoder");
    encoder.set_bitrate(Bitrate::BitsPerSecond(128_000)).expect("Set bitrate");

    let pcm = vec![0.0f32; 1920]; // 960 samples * 2 channels = 20ms
    let mut out = vec![0u8; 1024];
    let len = encoder.encode_float(&pcm, &mut out).expect("Encode silence");
    println!("Encoded 20ms of silence into {} bytes of Opus bitstream!", len);

    // Test a synthetic 440Hz sine wave
    let mut sine_pcm = Vec::with_capacity(1920);
    for i in 0..960 {
        let t = i as f32 / 48000.0;
        let s = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5;
        sine_pcm.push(s); // L
        sine_pcm.push(s); // R
    }
    let sine_len = encoder.encode_float(&sine_pcm, &mut out).expect("Encode sine");
    println!("Encoded 20ms of 440Hz stereo sine into {} bytes of Opus bitstream!", sine_len);
}
