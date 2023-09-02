use std::{fs::File, io::BufReader};

use criterion::{criterion_group, criterion_main, Criterion};
use flate2::{bufread, read};

fn decompress_tarball_buffered() {
    let tar_gz = BufReader::new(File::open("tests/test.tar.gz").unwrap());
    bufread::GzDecoder::new(tar_gz);
}

fn decompress_tarball() {
    let tar_gz = File::open("tests/test.tar.gz").unwrap();
    read::GzDecoder::new(tar_gz);
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut decompress = c.benchmark_group("decompress");
    decompress.sample_size(100);

    decompress.bench_function("decompress unbuffered", |b| b.iter(decompress_tarball));

    decompress.bench_function("decompress buffered", |b| {
        b.iter(decompress_tarball_buffered)
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
