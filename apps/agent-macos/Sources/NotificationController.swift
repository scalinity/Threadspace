import UserNotifications

/// The companion's `UNUserNotificationCenter` delegate, strongly held for the
/// process lifetime (see main.swift). Responses forward internal IDs only; the
/// core re-reads the item before anything is opened (SPEC §7.5).
final class NotificationController: NSObject, UNUserNotificationCenterDelegate {
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let request = response.notification.request
        let info = request.content.userInfo
        deliverToCore([
            "kind": "NotificationResponse",
            "schema": info["schema"] as? Int ?? 0,
            "requestId": request.identifier,
            "actionIdentifier": response.actionIdentifier,
            "attentionId": info["attentionId"] as? String ?? "",
            "sessionId": info["sessionId"] as? String ?? "",
        ])
        completionHandler()
    }
}
