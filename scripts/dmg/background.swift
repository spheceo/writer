// Writer's DMG background at 1x and 2x, in Edit's style. Run: swift scripts/dmg/background.swift <out-dir>
//
// Copied from Edit's scripts/dmg/background.swift (Oct 23 2026) and changed only where Writer
// differs: Writer's tones glow behind its icon (the light grey of its lines, up and left, and
// its red note, down and right) instead of Edit's three colours, and the words name Writer.
// Everything else is Edit's: black, the faint grid and vignette, the spotlights, the dotted arc,
// the light name tags and the instruction along the bottom.
// Finder writes icon names in dark grey over a background picture (and can't be
// told otherwise), so each name sits on a light tag drawn here; the Applications
// shortcut is named " ", its name and folder drawn here too (Finder may leave the
// shortcut's own icon blank on macOS 26).
// Content is 640×400 pt; settings.py puts Writer.app at (160, 186) and Applications at (480, 186).
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ".")
let W: CGFloat = 640, H: CGFloat = 400
let iconY: CGFloat = 186
let appX: CGFloat = 160, appsX: CGFloat = 480

func rgb(_ hex: UInt32, _ alpha: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
}
let fg = rgb(0xededed)
let faint = rgb(0x5c5c5c)

/// Points from the top-left (like Finder's icon positions) → AppKit's bottom-left.
func p(_ x: CGFloat, _ y: CGFloat) -> NSPoint { NSPoint(x: x, y: H - y) }

/// A soft round light: `color` at the centre fading to nothing at `radius`.
func glow(_ x: CGFloat, _ y: CGFloat, radius: CGFloat, color: NSColor) {
    let gradient = NSGradient(colors: [color, color.withAlphaComponent(color.alphaComponent * 0.45), color.withAlphaComponent(0)],
                              atLocations: [0, 0.45, 1], colorSpace: .sRGB)!
    gradient.draw(fromCenter: p(x, y), radius: 0, toCenter: p(x, y), radius: radius, options: [])
}

/// Text centred on x, its top edge at `top` from the top.
func text(_ s: String, size: CGFloat, weight: NSFont.Weight, color: NSColor, x: CGFloat, top: CGFloat) {
    let str = NSAttributedString(string: s, attributes: [.font: NSFont.systemFont(ofSize: size, weight: weight), .foregroundColor: color])
    let w = str.size()
    str.draw(at: NSPoint(x: x - w.width / 2, y: H - top - w.height))
}

/// The middle of the line Finder writes an icon's name on, measured off a real window.
let labelMid: CGFloat = iconY + 75
let tagText = rgb(0x262626)

/// The light tag a name sits on, centred under an icon.
func tag(width: CGFloat, x: CGFloat) {
    let rect = NSRect(x: x - width / 2, y: H - labelMid - 10, width: width, height: 20)
    NSColor.white.withAlphaComponent(0.88).setFill()
    NSBezierPath(roundedRect: rect, xRadius: 10, yRadius: 10).fill()
}

func render(scale: CGFloat) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(W * scale), pixelsHigh: Int(H * scale),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: W, height: H)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

    NSColor.black.setFill()
    NSRect(x: 0, y: 0, width: W, height: H).fill()

    // The grid, 32 pt squares, then a vignette that fades it out toward the edges.
    let grid = NSBezierPath()
    for x in stride(from: CGFloat(0), through: W, by: 32) { grid.move(to: NSPoint(x: x + 0.5, y: 0)); grid.line(to: NSPoint(x: x + 0.5, y: H)) }
    for y in stride(from: CGFloat(0), through: H, by: 32) { grid.move(to: NSPoint(x: 0, y: y + 0.5)); grid.line(to: NSPoint(x: W, y: y + 0.5)) }
    grid.lineWidth = 1 / scale
    NSColor.white.withAlphaComponent(0.06).setStroke()
    grid.stroke()
    let vignette = NSGradient(colors: [NSColor.black.withAlphaComponent(0), NSColor.black.withAlphaComponent(0.95)],
                              atLocations: [0.45, 1], colorSpace: .sRGB)!
    vignette.draw(fromCenter: p(W / 2, H * 0.45), radius: 0, toCenter: p(W / 2, H * 0.45), radius: W * 0.62, options: [.drawsAfterEndingLocation])

    // A spotlight under each icon (the icon is black: without it, it'd vanish).
    glow(appX, iconY, radius: 118, color: NSColor.white.withAlphaComponent(0.09))
    glow(appsX, iconY, radius: 118, color: NSColor.white.withAlphaComponent(0.09))
    // Writer's tones behind it: the light grey of its lines up and left, its red note down and right.
    glow(appX - 24, iconY - 24, radius: 96, color: rgb(0xededed, 0.20))
    glow(appX + 28, iconY + 26, radius: 86, color: rgb(0xff0033, 0.40))

    // The dotted arc from Writer to Applications, and its head.
    let arc = NSBezierPath()
    arc.move(to: p(238, 168))
    arc.curve(to: p(402, 168), controlPoint1: p(292, 134), controlPoint2: p(348, 134))
    arc.lineWidth = 2
    arc.lineCapStyle = .round
    arc.setLineDash([0.01, 8], count: 2, phase: 0)
    NSColor.white.withAlphaComponent(0.8).setStroke()
    arc.stroke()
    let head = NSBezierPath()
    // Along the arc's last stretch (control point → end), ±35°.
    head.move(to: p(397, 156))
    head.line(to: p(402, 168))
    head.line(to: p(389, 169))
    head.lineWidth = 2.2
    head.lineCapStyle = .round
    head.lineJoinStyle = .round
    NSColor.white.withAlphaComponent(0.9).setStroke()
    head.stroke()

    // The Applications folder and both names, where Finder would put them.
    let folder = NSWorkspace.shared.icon(forFile: "/Applications")
    folder.size = NSSize(width: 112, height: 112)
    folder.draw(in: NSRect(x: appsX - 56, y: H - iconY - 56, width: 112, height: 112))
    // Finder's own "Writer" is dark grey and can't be recoloured: it sits on a light tag, and Applications gets the same.
    tag(width: 58, x: appX)
    tag(width: 96, x: appsX)
    text("Applications", size: 13, weight: .regular, color: tagText, x: appsX, top: labelMid - 8)

    text("Drag Writer into Applications", size: 15, weight: .medium, color: fg, x: W / 2, top: 318)
    text("Then open it from Launchpad or Spotlight", size: 12, weight: .regular, color: faint, x: W / 2, top: 342)

    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

try! FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
try! render(scale: 1).write(to: out.appendingPathComponent("background.png"))
try! render(scale: 2).write(to: out.appendingPathComponent("background@2x.png"))
print("wrote \(out.path)/background.png and background@2x.png")
