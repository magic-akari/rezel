cl\u0061ss Unicode {
    int café = 1;
    int 变量 = 2;
    int 𐐀value = 3;
    int a‌b = 4;
    \u0069nt escaped = 5;
    // Unicode escape translation supplies the line terminator.\u000a    int translated = 6;

    String text() {
        return "🦀" + café + 变量 + 𐐀value + a‌b + escaped + translated;
    }
}
