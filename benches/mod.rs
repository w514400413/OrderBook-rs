mod simple;

use criterion::{criterion_group, criterion_main};
use simple::first::benchmark_data;

// Define the benchmark groups
criterion_group!(benches, benchmark_data,);

criterion_main!(benches);
