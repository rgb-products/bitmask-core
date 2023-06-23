mod rgb {

    mod unit {
        mod invoice;
        mod issue;
        mod psbt;
        mod stock;
        pub mod utils;
    }

    mod integration {
        // TODO: Review after support multi-token transfer
        // mod collectibles;
        mod collectibles;
        mod fungibles;
        mod issue;
        mod states;
        mod stress;
        mod udas;
        pub mod utils;
        mod watcher;
        mod import;
    }

    mod web {
        mod contracts;
        mod imports;
        mod std;
    }
}
