# Database Performance Testing

This directory contains tests for benchmarking different database implementations for storing and querying SPdmsElement data.

## DuckDB Test

The `test_duckdb.rs` file implements functionality to transfer SPdmsElement data from SurrealDB to DuckDB and benchmark query performance, especially for ORDER BY operations.

### Setup Instructions

To use the DuckDB tests properly, follow these steps:

1. **Install DuckDB**
   - Download DuckDB from: https://duckdb.org/docs/installation/index
   - Add the DuckDB library to your system path

2. **Configure Rust project**
   - Add the duckdb dependency to your Cargo.toml:
     ```toml
     [dependencies]
     duckdb = "0.10.0"
     ```

3. **Handle library linking**
   - Make sure DuckDB's shared library is available to your build system
   - For Windows: Place `duckdb.lib` in a location accessible to your linker, or set the `RUSTFLAGS` environment variable:
     ```
     set RUSTFLAGS=-L path/to/duckdb/lib
     ```
   - For Linux/macOS: Make sure `libduckdb.so`/`libduckdb.dylib` is in your library path

### Running the Tests

Once everything is set up, you can run the tests with:

```bash
cargo test --package aios_core --lib test::test_different_dbs::test_duckdb::tests::test_duckdb -- --nocapture
```

To create a persistent DuckDB database for further analysis:

```bash
cargo test --package aios_core --lib test::test_different_dbs::test_duckdb::tests::test_create_persistent_db -- --nocapture
```

## What to Expect

The tests will:

1. Fetch SPdmsElement data from SurrealDB's pe table
2. Store the data in DuckDB with appropriate indexes
3. Benchmark ORDER BY queries on different fields (name, noun, dbnum, sesno)
4. Compare the query performance with SurrealDB

## Performance Considerations

DuckDB is optimized for analytical queries and should generally perform better than SurrealDB for ORDER BY operations, especially on larger datasets. Here are some expected benefits:

- Faster sorting operations due to columnar storage
- Better query optimization for analytical workloads
- Efficient indexing for ORDER BY operations
- Lower memory consumption for large result sets

Note that the actual performance improvement can vary based on hardware, data size, and query complexity. 