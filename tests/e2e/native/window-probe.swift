import CoreGraphics
import Foundation

struct Window: Codable {
    let id: UInt32
    let ownerPID: Int
    let ownerName: String
    let title: String
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

let requestedPID = CommandLine.arguments.dropFirst().first.flatMap(Int.init)
let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let rawWindows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []

let windows = rawWindows.compactMap { raw -> Window? in
    let ownerPID = raw[kCGWindowOwnerPID as String] as? Int ?? -1
    if let requestedPID, requestedPID != ownerPID { return nil }
    let layer = raw[kCGWindowLayer as String] as? Int ?? -1
    guard layer == 0 else { return nil }
    guard
        let id = raw[kCGWindowNumber as String] as? UInt32,
        let bounds = raw[kCGWindowBounds as String] as? [String: Any]
    else { return nil }
    let width = bounds["Width"] as? Double ?? 0
    let height = bounds["Height"] as? Double ?? 0
    guard width >= 120, height >= 80 else { return nil }
    return Window(
        id: id,
        ownerPID: ownerPID,
        ownerName: raw[kCGWindowOwnerName as String] as? String ?? "",
        title: raw[kCGWindowName as String] as? String ?? "",
        x: bounds["X"] as? Double ?? 0,
        y: bounds["Y"] as? Double ?? 0,
        width: width,
        height: height
    )
}

let data = try JSONEncoder().encode(windows)
FileHandle.standardOutput.write(data)
