// Renders the app icon, the design in assets/icon.svg (a rose rounded
// square with a white "G"), as a macOS .iconset at every required size,
// using Apple's icon grid: an 824pt body with 100pt margins on 1024pt.
//
// usage: swift script/make-icon.swift <out.iconset>
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments[1])
try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

func render(_ pixels: Int) -> Data {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let scale = CGFloat(pixels) / 1024
    let body = NSRect(x: 100 * scale, y: 100 * scale, width: 824 * scale, height: 824 * scale)

    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.25)
    shadow.shadowOffset = NSSize(width: 0, height: -10 * scale)
    shadow.shadowBlurRadius = 20 * scale
    shadow.set()
    NSColor(red: 0xe1 / 255, green: 0x1d / 255, blue: 0x48 / 255, alpha: 1).setFill()
    NSBezierPath(roundedRect: body, xRadius: 185 * scale, yRadius: 185 * scale).fill()
    NSShadow().set()

    let font = NSFont.systemFont(ofSize: 520 * scale, weight: .semibold)
    let text = NSAttributedString(string: "G", attributes: [.font: font, .foregroundColor: NSColor.white])
    let size = text.size()
    text.draw(at: NSPoint(x: body.midX - size.width / 2, y: body.midY - size.height / 2))

    NSGraphicsContext.current = nil
    return rep.representation(using: .png, properties: [:])!
}

for points in [16, 32, 128, 256, 512] {
    for factor in [1, 2] {
        let name = factor == 1 ? "icon_\(points)x\(points).png" : "icon_\(points)x\(points)@2x.png"
        try render(points * factor).write(to: out.appendingPathComponent(name))
    }
}
