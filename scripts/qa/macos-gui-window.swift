import AppKit
import CoreGraphics
import Darwin
let pid=Int(CommandLine.arguments[1])!
let windows=CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],kCGNullWindowID) as? [[String:Any]] ?? []
let own=windows.filter { ($0[kCGWindowOwnerPID as String] as? Int)==pid && ($0[kCGWindowLayer as String] as? Int)==0 }
let bounds=own.compactMap { $0[kCGWindowBounds as String] as? [String:Any] }.filter { ($0["Width"] as? Double ?? 0)>=960 && ($0["Height"] as? Double ?? 0)>=600 }
if bounds.isEmpty {fputs("No visible main application window for PID \(pid)\n",stderr);exit(1)}
let result:[String:Any] = ["visible_main_windows":bounds.count,"bounds":bounds]
let data=try JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys])
FileHandle.standardOutput.write(data)
