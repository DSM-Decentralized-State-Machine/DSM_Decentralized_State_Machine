use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pqcrypto_mlkem::mlkem768 as kyber; // Use concrete implementation instead
use pqcrypto_sphincsplus::sphincssha2128ssimple as sphincs; // Use correct implementation
                                                            // use rand::prelude::*; // Commenting out as unused
use std::time::Duration;

mod bench;

/// Benchmarks for quantum-resistant cryptographic operations in DSM
///
/// This suite evaluates the performance characteristics of post-quantum cryptographic
/// primitives used in DSM, providing insights into their computational costs
/// for various security parameter choices. These benchmarks directly inform
/// trade-off decisions between security levels and operational performance.
fn quantum_crypto_benchmark(c: &mut Criterion) {
    // Initialize DSM for consistent benchmark environment
    dsm::initialize();

    let mut group = c.benchmark_group("Quantum-Resistant Cryptography");
    group.sample_size(100);
    group.measurement_time(Duration::from_secs(10));

    // Kyber KEM benchmarks for different security levels
    // Benchmark key generation
    group.bench_function("kyber_keygen_768", |b| {
        b.iter(|| black_box(kyber::keypair()))
    });

    // Benchmark encapsulation
    group.bench_function("kyber_encapsulate_768", |b| {
        let (public_key, _) = kyber::keypair();

        b.iter(|| black_box(kyber::encapsulate(&public_key)))
    });

    // Benchmark decapsulation
    group.bench_function("kyber_decapsulate_768", |b| {
        let (public_key, private_key) = kyber::keypair();
        let (_, ciphertext) = kyber::encapsulate(&public_key);

        b.iter(|| black_box(kyber::decapsulate(&ciphertext, &private_key)))
    });

    // SPHINCS+ benchmarks
    group.bench_function("sphincs_keygen", |b| {
        b.iter(|| black_box(sphincs::keypair()))
    });

    group.bench_function("sphincs_sign", |b| {
        let (_, private_key) = sphincs::keypair();
        let message = b"This is a test message for SPHINCS+ signature benchmark";

        b.iter(|| black_box(sphincs::sign(message, &private_key)))
    });

    group.bench_function("sphincs_verify", |b| {
        let (public_key, private_key) = sphincs::keypair();
        let message = b"This is a test message for SPHINCS+ signature benchmark";
        let signed_message = sphincs::sign(message, &private_key);

        b.iter(|| black_box(sphincs::open(&signed_message, &public_key)));
    });

    // Hybrid quantum resistance benchmarks (combining Kyber and SPHINCS+)
    group.bench_function("hybrid_qr_key_exchange", |b| {
        b.iter(|| {
            // Generate SPHINCS+ keypair for authentication
            let (sphincs_pk, sphincs_sk) = sphincs::keypair();

            // Generate Kyber keypair for encryption
            let (kyber_pk, kyber_sk) = kyber::keypair();

            // Use pqcrypto_traits implementation
            use pqcrypto_traits::kem::PublicKey;
            // Serialize Kyber public key as bytes for signing
            let kyber_pk_bytes = kyber_pk.as_bytes();

            // Simulate authenticated key exchange
            // 1. Sign Kyber public key with SPHINCS+
            let signed_kyber_pk = sphincs::sign(kyber_pk_bytes, &sphincs_sk);

            // Verify signature using the proper function
            let verified = match sphincs::open(&signed_kyber_pk, &sphincs_pk) {
                Ok(msg) if msg == kyber_pk_bytes => Ok(()),
                _ => Err(()),
            };

            // 3. Perform Kyber key encapsulation if verified
            let result = if verified.is_ok() {
                let (_, cipher) = kyber::encapsulate(&kyber_pk);
                let _ = kyber::decapsulate(&cipher, &kyber_sk);
                true
            } else {
                false
            };

            black_box(result)
        })
    });

    group.finish();
}

criterion_group!(quantum_crypto_benches, quantum_crypto_benchmark);
criterion_main!(quantum_crypto_benches);
