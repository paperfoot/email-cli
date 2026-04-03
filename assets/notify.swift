import Cocoa
import UserNotifications

// Needed so the app stays alive long enough for the permission dialog
let app = NSApplication.shared

let args = CommandLine.arguments
let title = args.count > 1 ? args[1] : ""
let subtitle = args.count > 2 ? args[2] : ""
let body = args.count > 3 ? args[3] : ""

let center = UNUserNotificationCenter.current()

center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
    if !granted {
        fputs("Notification permission not granted. Enable in System Settings > Notifications > Email CLI\n", stderr)
        DispatchQueue.main.async { app.terminate(nil) }
        return
    }

    let content = UNMutableNotificationContent()
    content.title = title
    content.subtitle = subtitle
    content.body = body
    content.sound = UNNotificationSound(named: UNNotificationSoundName("EmailCLI.aiff"))

    let request = UNNotificationRequest(
        identifier: UUID().uuidString,
        content: content,
        trigger: nil
    )

    center.add(request) { error in
        if let error = error {
            fputs("Notification error: \(error)\n", stderr)
        }
        DispatchQueue.main.async { app.terminate(nil) }
    }
}

app.run()
