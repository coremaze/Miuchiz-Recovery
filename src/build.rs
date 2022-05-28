fn main() {
    let src = [
        "Native-Miuchiz-Handheld-USB-Utilities/libmiuchiz-usb/src/commands.c",
        "Native-Miuchiz-Handheld-USB-Utilities/libmiuchiz-usb/src/libmiuchiz-usb.c",
        "Native-Miuchiz-Handheld-USB-Utilities/libmiuchiz-usb/src/timer.c",
    ];
    let mut builder = cc::Build::new();
    let build = builder
        .files(src.iter())
        .include("Native-Miuchiz-Handheld-USB-Utilities/libmiuchiz-usb/include")
        .flag("-Wno-sign-compare")
        .flag("-Wno-extra");
    build.compile("libmiuchiz-usb");
}
