fn main() {
    // esp-idf-sys builds ESP-IDF and records the resulting link/search flags in
    // its sysenv; emit them here so ldproxy forwards them to the final link.
    embuild::espidf::sysenv::output();
}
