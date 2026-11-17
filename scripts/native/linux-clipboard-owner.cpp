// Native Qt Wayland clipboard probe. Content crosses only private pipes and RAM.
#include <QApplication>
#include <QClipboard>
#include <QWidget>
#include <QMimeData>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonArray>
#include <QSocketNotifier>
#include <iostream>

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    app.setQuitOnLastWindowClosed(false);
    QWidget window;
    window.setWindowTitle("Rodtty clipboard check");
    window.resize(180, 60);
    window.show();
    QJsonObject saved;
    QSocketNotifier input(0, QSocketNotifier::Read);
    auto content = [&]() {
        QJsonObject data;
        const auto *mime = app.clipboard()->mimeData();
        if (mime) for (const auto &name : mime->formats())
            data.insert(name, QString::fromLatin1(mime->data(name).toBase64()));
        return data;
    };
    auto publish = [&](const QJsonObject &data) {
        auto *mime = new QMimeData;
        for (auto i = data.begin(); i != data.end(); ++i)
            mime->setData(i.key(), QByteArray::fromBase64(i.value().toString().toLatin1()));
        app.clipboard()->setMimeData(mime);
    };
    QObject::connect(&input, &QSocketNotifier::activated, [&]() {
        std::string line;
        if (!std::getline(std::cin, line)) {
            input.setEnabled(false);
            app.quit();
            return;
        }
        auto command = QJsonDocument::fromJson(QByteArray::fromStdString(line)).object();
        const bool fixture = app.clipboard()->text() == QString::fromUtf8("Olá 世界 🦀");
        QJsonObject result{{"active", app.applicationState() == Qt::ApplicationActive}, {"is_fixture", fixture}};
        if (command.contains("snapshot")) {
            saved = content();
            result.insert("content", saved);
        }
        if (command.contains("set_fixture") && fixture && !saved.isEmpty()) {
            app.clipboard()->setText(QString::fromUtf8("Olá 世界 🦀"));
            result.insert("fixture_set", true);
        }
        if (command.contains("restore") && fixture && !saved.isEmpty()) {
            publish(saved);
            result.insert("restored", true);
        }
        if (command.contains("verify")) {
            const auto expected = command.value("verify").toObject();
            const auto current = content();
            bool matches = !expected.isEmpty();
            for (auto i = expected.begin(); i != expected.end(); ++i)
                matches = matches && current.value(i.key()) == i.value();
            result.insert("original_forms_match", matches);
        }
        QJsonArray formats;
        for (const auto &name : content().keys()) formats.append(name);
        result.insert("formats", formats);
        std::cout << QJsonDocument(result).toJson(QJsonDocument::Compact).toStdString() << std::endl;
    });
    std::cout << "ready" << std::endl;
    return app.exec();
}
