#include "../src/network/texttranslationprotocol.h"
#include <QCoreApplication>
#include <QUrlQuery>
#include <iostream>
#include <cstdlib>

using namespace snow_shot;
namespace {
void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(1);
    }
}
TextTranslationConfiguration configuration() {
    return {QStringLiteral("b7d7fe03-8878-4ec1-8f88-a9caf005530c"),
            QStringLiteral("Lite"),
            QStringLiteral("youdao-lite"),
            QStringLiteral("https://translator.invalid/translate"),
            QStringLiteral("fixture-secret"),
            QStringLiteral("fixture-app"),
            4};
}
void requestContract() {
    const auto config = configuration();
    bool valid = false;
    const auto roundTrip = textTranslationConfigurationsFromJson(
        textTranslationConfigurationsToJson({config}), &valid);
    require(valid && roundTrip.size() == 1 && roundTrip.first() == config,
            "Lite configuration round-trips without changing identity or credentials");
    for (const auto& text : {QStringLiteral("hello + & = %\nworld"),
                             QString::fromUcs4(U"0123456789abcdefghij\U0001F600")}) {
        const QUrlQuery form(QString::fromUtf8(text_translation::body(
            config, {config.selectionId(), QStringLiteral("auto"), QStringLiteral("zh-Hant"), text},
            QStringLiteral("fixture-salt"), QStringLiteral("123"))));
        const auto field = [&form](const QString& name) {
            return form.queryItemValue(name, QUrl::FullyDecoded);
        };
        require(field(QStringLiteral("i")) == text && !form.hasQueryItem(QStringLiteral("q")),
                "Lite uses i, preserves Unicode and form metacharacters, and never sends NMT q");
        require(field(QStringLiteral("handleOption")) == QStringLiteral("3") &&
                    field(QStringLiteral("streamType")) == QStringLiteral("increment") &&
                    field(QStringLiteral("to")) == QStringLiteral("zh-CHT") &&
                    field(QStringLiteral("signType")) == QStringLiteral("v3") &&
                    !form.hasQueryItem(QStringLiteral("strict")),
                "Lite selector, stream mode, language and signature version");
        const auto points = text.toStdU32String();
        const auto input = points.size() <= 20
                               ? text
                               : QString::fromUcs4(points.data(), 10) +
                                     QString::number(points.size()) +
                                     QString::fromUcs4(points.data() + points.size() - 10, 10);
        const auto expected =
            QCryptographicHash::hash(
                (config.applicationId + input + QStringLiteral("fixture-salt123") + config.apiKey)
                    .toUtf8(),
                QCryptographicHash::Sha256)
                .toHex();
        require(field(QStringLiteral("sign")).toLatin1() == expected,
                "Lite v3 signs Unicode code points rather than UTF-16 units");
    }
}
void eventContract() {
    const auto parse = [](const QByteArray& bytes) {
        return text_translation::parseYoudaoLiteEvent(QJsonDocument::fromJson(bytes).object());
    };
    auto event = parse(R"({"code":"0","successful":true,"data":{"transIncre":"hello"}})");
    require(event.valid && event.text == QStringLiteral("hello") && event.code.isEmpty(),
            "incremental translation event");
    event = parse(R"({"code":"0","successful":true,"data":{"transIncre":""}})");
    require(event.valid && event.text.isEmpty(), "empty increments are valid frames");
    for (const auto& bytes :
         {QByteArrayLiteral("{}"),
          QByteArrayLiteral(R"({"code":"0","successful":false,"data":{"transIncre":"bad"}})"),
          QByteArrayLiteral(R"({"code":"0","successful":true,"data":{"transIncre":42}})"),
          QByteArrayLiteral(R"({"code":"0","successful":true,"data":{"transFull":"wrong mode"}})"),
          QByteArrayLiteral(R"({"code":"https://secret.invalid/","message":"fixture-secret"})")}) {
        event = parse(bytes);
        require(!event.valid && event.text.isEmpty() && event.code.isEmpty(),
                "malformed or unsuccessful events cannot publish text or arbitrary error details");
    }
    event = parse(R"({"code":"202","message":"fixture-secret"})");
    require(!event.valid && event.code == QStringLiteral("202") && !event.throttled,
            "signature errors retain only their numeric code");
    for (const auto* code : {"411", "412", "429"}) {
        event = parse(QByteArrayLiteral("{\"code\":\"") + code + "\"}");
        require(event.throttled && !event.valid, "rate limit codes normalize to throttling");
    }
}
} // namespace
int main(int argc, char** argv) {
    QCoreApplication app(argc, argv);
    requestContract();
    eventContract();
    std::cout << "Youdao Lite protocol tests passed\n";
    return 0;
}
