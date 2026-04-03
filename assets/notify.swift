import Cocoa

let args = CommandLine.arguments
let title = args.count > 1 ? args[1] : ""
let subtitle = args.count > 2 ? args[2] : ""
let body = args.count > 3 ? args[3] : ""

let notification = NSUserNotification()
notification.title = title
notification.subtitle = subtitle
notification.informativeText = body
notification.soundName = NSUserNotificationDefaultSoundName

// Set custom icon if provided
if args.count > 4 {
    if let image = NSImage(contentsOfFile: args[4]) {
        notification.contentImage = image
    }
}

NSUserNotificationCenter.default.deliver(notification)
