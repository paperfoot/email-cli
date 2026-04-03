import Cocoa
import UserNotifications

let args = CommandLine.arguments
let title = args.count > 1 ? args[1] : ""
let subtitle = args.count > 2 ? args[2] : ""
let body = args.count > 3 ? args[3] : ""

let semaphore = DispatchSemaphore(value: 0)
let center = UNUserNotificationCenter.current()

center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
    guard granted else {
        fputs("Notification permission denied\n", stderr)
        semaphore.signal()
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
        semaphore.signal()
    }
}

_ = semaphore.wait(timeout: .now() + 5)
