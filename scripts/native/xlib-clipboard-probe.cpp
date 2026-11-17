// Independent XCB clipboard consumer/provider for an initially empty X selection.
// The harness refuses this probe when X owns any prior content; Wayland owner
// content is independently snapshotted/restored through native data-control.
#include <QApplication>
#include <QClipboard>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMimeData>
#include <QSocketNotifier>
#include <iostream>
int main(int argc, char **argv) {
    QApplication app(argc, argv);
    QSocketNotifier input(0, QSocketNotifier::Read);
    QObject::connect(&input, &QSocketNotifier::activated, [&]() {
        std::string line;
        if (!std::getline(std::cin, line)) { app.quit(); return; }
        auto command = QJsonDocument::fromJson(QByteArray::fromStdString(line)).object();
        const auto text = app.clipboard()->text();
        const bool copy = text == QString::fromUtf8("Olá 世界 🦀");
        QJsonObject result{{"clipboard_access", true}, {"is_copy_fixture", copy}};
        if (command.contains("set_fixture") && copy) {
            app.clipboard()->setText(QString::fromUtf8("Olá 世界 🐚"));
            result.insert("fixture_set", true);
        }
        std::cout << QJsonDocument(result).toJson(QJsonDocument::Compact).toStdString() << std::endl;
    });
    std::cout << "ready" << std::endl;
    return app.exec();
}
