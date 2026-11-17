// Native data-control test helper: all clipboard payloads stay in RAM/private pipes.
// Unlike normal Wayland set_selection, data-control does not need an input serial.
#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <wayland-client.h>
#include "clipboard-data-control-client.h"
#include <algorithm>
#include <cerrno>
#include <csignal>
#include <fcntl.h>
#include <iostream>
#include <memory>
#include <poll.h>
#include <string>
#include <thread>
#include <unistd.h>
#include <vector>

struct Offer { ext_data_control_offer_v1 *proxy; std::vector<std::string> types; };
struct Source { QJsonObject content; bool cancelled = false; };
static wl_display *display;
static wl_seat *seat;
static ext_data_control_manager_v1 *manager;
static Offer *current;
static std::vector<std::unique_ptr<Offer>> offers;
static std::vector<std::unique_ptr<Source>> sources;
static Source *owned;
static const char *control = "application/x-kde-onlyReplaceEmpty";
static void offered(void *data, ext_data_control_offer_v1 *, const char *mime) {
    static_cast<Offer *>(data)->types.emplace_back(mime);
}
static const ext_data_control_offer_v1_listener offer_listener = {offered};
static void data_offer(void *, ext_data_control_device_v1 *, ext_data_control_offer_v1 *proxy) {
    auto offer = std::make_unique<Offer>(); offer->proxy = proxy;
    ext_data_control_offer_v1_add_listener(proxy, &offer_listener, offer.get());
    offers.push_back(std::move(offer));
}
static void selection(void *, ext_data_control_device_v1 *, ext_data_control_offer_v1 *proxy) {
    current = nullptr;
    for (const auto &offer : offers) if (offer->proxy == proxy) current = offer.get();
}
static void finished(void *, ext_data_control_device_v1 *) { current = nullptr; }
static void primary(void *, ext_data_control_device_v1 *, ext_data_control_offer_v1 *) {}
static const ext_data_control_device_v1_listener device_listener = {data_offer, selection, finished, primary};
static void global(void *, wl_registry *registry, uint32_t id, const char *interface, uint32_t version) {
    if (std::string(interface) == "wl_seat") seat = static_cast<wl_seat *>(wl_registry_bind(registry, id, &wl_seat_interface, 1));
    if (std::string(interface) == "ext_data_control_manager_v1") manager = static_cast<ext_data_control_manager_v1 *>(wl_registry_bind(registry, id, &ext_data_control_manager_v1_interface, std::min(version, 1u)));
}
static void removed(void *, wl_registry *, uint32_t) {}
static const wl_registry_listener registry_listener = {global, removed};
static void send_data(void *data, ext_data_control_source_v1 *, const char *mime, int fd) {
    const auto bytes = QByteArray::fromBase64(static_cast<Source *>(data)->content.value(QString::fromUtf8(mime)).toString().toLatin1()).toStdString();
    std::thread([bytes, fd]() {
        size_t offset = 0;
        while (offset < bytes.size()) {
            auto n = write(fd, bytes.data() + offset, bytes.size() - offset);
            if (n > 0) offset += static_cast<size_t>(n);
            else if (n < 0 && errno == EINTR) continue;
            else break;
        }
        close(fd);
    }).detach();
}
static void cancelled(void *data, ext_data_control_source_v1 *) { static_cast<Source *>(data)->cancelled = true; }
static const ext_data_control_source_v1_listener source_listener = {send_data, cancelled};
static bool content(QJsonObject &result) {
    if (owned && !owned->cancelled) { result = owned->content; return true; }
    Offer *offer = current;
    if (!offer) return false;
    for (const auto &type : offer->types) {
        // This is a manager protocol instruction, not owner data. Re-publishing
        // it tells Klipper to reject restoring over our nonempty fixture.
        // Every other MIME, including KDE cut/file semantics, is preserved.
        if (type == control) continue;
        int fds[2]; if (pipe(fds) != 0) return false;
        fcntl(fds[0], F_SETFL, O_NONBLOCK);
        ext_data_control_offer_v1_receive(offer->proxy, type.c_str(), fds[1]);
        close(fds[1]); wl_display_flush(display);
        QByteArray bytes; bool done = false;
        while (!done) {
            pollfd ready[2] = {{fds[0], POLLIN, 0}, {wl_display_get_fd(display), POLLIN, 0}};
            if (poll(ready, 2, 5000) <= 0) { close(fds[0]); return false; }
            if (ready[1].revents & POLLIN) {
                if (wl_display_dispatch(display) < 0) { close(fds[0]); return false; }
            }
            if (ready[0].revents & (POLLIN | POLLHUP)) {
                char buffer[65536]; ssize_t count;
                while ((count = read(fds[0], buffer, sizeof(buffer))) > 0) bytes.append(buffer, static_cast<int>(count));
                done = count == 0;
            }
        }
        close(fds[0]);
        if (current != offer) return false; // Never snapshot across an owner change.
        result.insert(QString::fromStdString(type), QString::fromLatin1(bytes.toBase64()));
    }
    return true;
}
static void publish(ext_data_control_device_v1 *device, const QJsonObject &data) {
    auto source = std::make_unique<Source>(); source->content = data;
    auto *proxy = ext_data_control_manager_v1_create_data_source(manager);
    ext_data_control_source_v1_add_listener(proxy, &source_listener, source.get());
    for (auto i = data.begin(); i != data.end(); ++i) ext_data_control_source_v1_offer(proxy, i.key().toUtf8().constData());
    owned = source.get(); sources.push_back(std::move(source));
    ext_data_control_device_v1_set_selection(device, proxy);
    wl_display_roundtrip(display);
}
int main() {
    std::signal(SIGPIPE, SIG_IGN);
    display = wl_display_connect(nullptr); if (!display) return 1;
    auto *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, nullptr); wl_display_roundtrip(display);
    if (!seat || !manager) return 2;
    auto *device = ext_data_control_manager_v1_get_data_device(manager, seat);
    ext_data_control_device_v1_add_listener(device, &device_listener, nullptr);
    wl_display_roundtrip(display); wl_display_roundtrip(display);
    QJsonObject saved;
    std::cout << "ready" << std::endl;
    while (true) {
        wl_display_dispatch_pending(display); wl_display_flush(display);
        pollfd ready[2] = {{0, POLLIN, 0}, {wl_display_get_fd(display), POLLIN, 0}};
        if (poll(ready, 2, -1) <= 0) continue;
        if (ready[1].revents & POLLIN) if (wl_display_dispatch(display) < 0) break;
        if (!(ready[0].revents & (POLLIN | POLLHUP))) continue;
        std::string line; if (!std::getline(std::cin, line)) break;
        auto command = QJsonDocument::fromJson(QByteArray::fromStdString(line)).object();
        QJsonObject now; bool readable = content(now);
        const auto fixture = QString::fromUtf8("Olá 世界 🦀").toUtf8();
        bool is_fixture = QByteArray::fromBase64(now.value("text/plain").toString().toLatin1()) == fixture;
        QJsonObject result{{"clipboard_access", readable}, {"is_fixture", is_fixture}};
        if (command.contains("snapshot") && readable) { saved = now; result.insert("content", saved); }
        if (command.contains("set_fixture") && is_fixture && !saved.isEmpty()) {
            const auto encoded = QString::fromLatin1(fixture.toBase64());
            publish(device, QJsonObject{{"text/plain", encoded}, {"text/plain;charset=utf-8", encoded}});
            result.insert("fixture_set", true);
        }
        if (command.contains("restore") && is_fixture && !saved.isEmpty()) { publish(device, saved); result.insert("restored", true); }
        // Recovery accepts only a known fixture, never an unrelated owner update.
        if (command.contains("recover") && is_fixture) { publish(device, command.value("recover").toObject()); result.insert("restored", true); }
        QJsonObject after; content(after);
        if (command.contains("verify")) {
            const auto expected = command.value("verify").toObject(); bool matches = !expected.isEmpty();
            for (auto i = expected.begin(); i != expected.end(); ++i) matches = matches && after.value(i.key()) == i.value();
            result.insert("original_forms_match", matches);
        }
        QJsonArray formats; for (const auto &name : after.keys()) formats.append(name);
        result.insert("formats", formats);
        std::cout << QJsonDocument(result).toJson(QJsonDocument::Compact).toStdString() << std::endl;
    }
    wl_display_disconnect(display); return 0;
}
