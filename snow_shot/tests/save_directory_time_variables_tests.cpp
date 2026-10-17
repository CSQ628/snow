// SPDX-License-Identifier: GPL-3.0-only
#include "snow_shot/presentation/screenshotimagefileservice.h"
#include "snow_shot/presentation/screenshotmanualsave.h"
#include "snow_shot/presentation/screenrecordingfolder.h"
#include "snow_shot/storage/applicationstorage.h"
#include "snow_shot/storage/timevariables.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QTemporaryDir>
#include <QTimeZone>

#include <cstdlib>
#include <iostream>

namespace storage = snow_shot::storage;
namespace recording = snow_shot::presentation::recording;

namespace {
void require(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(EXIT_FAILURE);
    }
}

void windowVariables(const QTemporaryDir& temporary) {
    const QDateTime timestamp(QDate(2026, 10, 16), QTime(12, 34, 56), QTimeZone::UTC);
    const storage::ScreenshotWindowNames names{QStringLiteral("Google Chrome"),
                                               QStringLiteral("文档: A/B")};
    require(storage::containsScreenshotWindowVariables(QStringLiteral("/{AN}/image")) &&
                storage::containsScreenshotWindowVariables(QStringLiteral("{WN}")) &&
                !storage::containsScreenshotWindowVariables(QStringLiteral("{an}-{wn}-{YYYY}")),
            "only exact AN and WN tokens must trigger native lookup");
    require(storage::expandScreenshotExportVariables(QStringLiteral("{AN}/{WN}_{YYYY}_{AN}"),
                                                     timestamp, names) ==
                QStringLiteral("Google Chrome/文档_ A_B_2026_Google Chrome"),
            "window values must be sanitized individually and coexist with time variables");
    require(storage::expandScreenshotExportVariables(QStringLiteral("{AN}-{WN}"), timestamp) ==
                    QStringLiteral("Unknown-Unknown") &&
                storage::expandScreenshotExportVariables(QStringLiteral("{AN}-{WN}"), timestamp,
                                                         {QStringLiteral("Known"), {}}) ==
                    QStringLiteral("Known-Unknown"),
            "missing names must fall back independently");
    require(storage::expandScreenshotExportVariables(
                QStringLiteral("{AN}-{WN}"), timestamp,
                {QStringLiteral("{WN}"), QStringLiteral("{YYYY}")}) ==
                QStringLiteral("{WN}-{YYYY}"),
            "retrieved names must never be parsed as templates");
    for (const QString& value : {QStringLiteral("CON"), QStringLiteral("nul.txt"),
                                 QStringLiteral("LPT1"), QStringLiteral("COM²")})
        require(storage::expandScreenshotExportVariables(
                    QStringLiteral("{WN}"), timestamp, {{}, value}) == QStringLiteral("_") + value,
                "reserved device names must be safe path components");
    require(storage::expandScreenshotExportVariables(
                QStringLiteral("{AN}/{WN}"), timestamp,
                {QStringLiteral(" .. "), QStringLiteral("x\n\t\x7f<>:\"/\\|?*... ")}) ==
                QStringLiteral("Unknown/x____________"),
            "dot-only names, control characters and path punctuation must be safe");
    require(storage::expandTimeVariables(QStringLiteral("{AN}-{WN}"), timestamp) ==
                timestamp.toString(QStringLiteral("AN")) + QLatin1Char('-') +
                    timestamp.toString(QStringLiteral("WN")),
            "time-only expansion must retain recording compatibility");
    const QString directory = temporary.filePath(QStringLiteral("names/{AN}/{YYYY}"));
    const QString filename = QStringLiteral("{WN}_{HH-mm-ss}");
    require(storage::ScreenshotSettings().setImageSaveDirectory(directory) &&
                storage::ScreenshotSettings().setManualSaveFilenameFormat(filename) &&
                storage::ScreenshotSettings().setLastManualSaveDirectory({}),
            "window naming settings must persist");
    QImage image(2, 2, QImage::Format_RGB32);
    image.fill(Qt::red);
    const auto directories =
        ScreenshotImageFileService::automaticDirectories(directory, timestamp, names);
    const auto saved = ScreenshotImageFileService::saveAutomatically(
        image, directories, ScreenshotImageFileFormat::Png, filename, timestamp, {}, {}, names);
    require(saved.succeeded() &&
                saved.path == screenshotManualSaveDefaults(timestamp, names).initialPath &&
                saved.path.endsWith(QStringLiteral("/Google Chrome/2026/文档_ A_B_12-34-56.png")),
            "automatic saves and native dialog defaults must use identical metadata");
    const auto duplicate = ScreenshotImageFileService::saveAutomatically(
        image, directories, ScreenshotImageFileFormat::Png, filename, timestamp, {}, {}, names);
    require(duplicate.succeeded() && duplicate.path.endsWith(QStringLiteral("_1.png")),
            "window variables must retain collision suffix behavior");
}

void timeDirectories(const QTemporaryDir& temporary) {
    const QDateTime before(QDate(2026, 12, 31), QTime(23, 59, 59), QTimeZone::UTC);
    const QDateTime after = before.addSecs(1);
    const QString pattern = temporary.filePath(QStringLiteral("output/{YYYY}/{MM}/{DD}"));
    require(storage::expandTimeVariables(QStringLiteral(" {YYYY-MM-DD_HH-mm-ss} "), before) ==
                    QStringLiteral("2026-12-31_23-59-59") &&
                storage::expandTimeVariables(QStringLiteral("{yyyy}/{YY}/{dd}"), after) ==
                    QStringLiteral("2027/27/01"),
            "shared expansion must preserve existing date formats and aliases");
    require(
        storage::ScreenshotSettings().setImageSaveDirectory(pattern) &&
            storage::ScreenshotSettings().setManualSaveFilenameFormat(QStringLiteral("{YYYY}")) &&
            storage::RecordingSettings().setVideoSaveDirectory(pattern),
        "template settings must persist");
    QImage image(QSize(2, 2), QImage::Format_RGB32);
    image.fill(Qt::red);
    for (const auto& timestamp : {before, after}) {
        const QString expanded = storage::expandTimeVariables(pattern, timestamp);
        require(!QFileInfo::exists(expanded), "dated directory must initially be absent");
        const auto imageDirectories =
            ScreenshotImageFileService::automaticDirectories(pattern, timestamp);
        const auto videoDirectories = recording::screenRecordingDirectories(timestamp);
        require(imageDirectories.constFirst() == expanded &&
                    videoDirectories.constFirst() == expanded && !QFileInfo::exists(expanded),
                "directory resolution must expand templates without filesystem mutation");
        const auto saved = ScreenshotImageFileService::saveAutomatically(
            image, imageDirectories, ScreenshotImageFileFormat::Png, QStringLiteral("{YYYY}"),
            timestamp);
        const auto defaults = screenshotManualSaveDefaults(timestamp);
        require(
            saved.succeeded() && saved.path == defaults.initialPath &&
                saved.path == QDir(expanded).filePath(timestamp.toString(QStringLiteral("yyyy")) +
                                                      QStringLiteral(".png")) &&
                !saved.path.contains(QLatin1Char('{')),
            "automatic and manual output must use one timestamp and create the dated directory");
        const QString baseName =
            ScreenshotImageFileService::suggestedBaseName(QStringLiteral("{YYYY}"), timestamp);
        const QString video =
            recording::screenRecordingOutputPath(videoDirectories, baseName, QStringLiteral("mp4"));
        require(video == QDir(expanded).filePath(baseName + QStringLiteral(".mp4")) &&
                    recording::screenRecordingDirectory(timestamp) == expanded,
                "recording output and folder resolution must use the expanded directory");
        QFile collision(video);
        require(collision.open(QIODevice::WriteOnly), "video collision fixture must open");
        collision.close();
        require(recording::screenRecordingOutputPath(videoDirectories, baseName,
                                                     QStringLiteral("mp4")) ==
                    QDir(expanded).filePath(baseName + QStringLiteral("_1.mp4")),
                "recording collision names must remain intact");
    }
    require(storage::ScreenshotSettings().imageSaveDirectory() == pattern &&
                storage::RecordingSettings().videoSaveDirectory() == pattern,
            "resolution must preserve stored templates");

    const QString videoPattern = temporary.filePath(QStringLiteral("videos/{YYYY}"));
    const QString videoDirectory = temporary.filePath(QStringLiteral("videos/2027"));
    require(storage::RecordingSettings().setVideoSaveDirectory(videoPattern) &&
                !QFileInfo::exists(videoDirectory),
            "new recording directory fixture must be absent");
    require(recording::screenRecordingOutputPath(recording::screenRecordingDirectories(after),
                                                 QStringLiteral("video"), QStringLiteral("mp4")) ==
                    QDir(videoDirectory).filePath(QStringLiteral("video.mp4")) &&
                QFileInfo(videoDirectory).isDir(),
            "first recording must create its dated output directory");

    const QString remembered = temporary.filePath(QStringLiteral("remembered/{YYYY}"));
    require(QDir().mkpath(remembered) && ScreenshotImageFileService::saveDialogDirectory(
                                             remembered, pattern, before) == remembered,
            "remembered concrete paths must remain literal and retain precedence");
    const QString missing = temporary.filePath(QStringLiteral("missing"));
    require(!ScreenshotImageFileService::automaticDirectories(missing, before).contains(missing) &&
                storage::RecordingSettings().setVideoSaveDirectory(missing) &&
                !recording::screenRecordingDirectories(before).contains(missing),
            "missing literal configured directories must retain their fallback behavior");
    const QString blocker = temporary.filePath(QStringLiteral("blocked"));
    QFile file(blocker);
    require(file.open(QIODevice::WriteOnly), "blocked directory fixture must open");
    file.close();
    const QString expandedBlocked =
        storage::expandTimeVariables(blocker + QStringLiteral("/{YYYY}"), before);
    const QString fallback = temporary.filePath(QStringLiteral("fallback"));
    const auto saved = ScreenshotImageFileService::saveAutomatically(
        image, {expandedBlocked, fallback}, ScreenshotImageFileFormat::Png,
        QStringLiteral("fallback"), before);
    require(saved.succeeded() && QFileInfo(saved.path).absolutePath() == fallback &&
                recording::screenRecordingOutputPath({expandedBlocked, fallback},
                                                     QStringLiteral("fallback"),
                                                     QStringLiteral("mp4")) ==
                    QDir(fallback).filePath(QStringLiteral("fallback.mp4")),
            "unusable expanded targets must retain writer fallback behavior");
}
} // namespace

int main(int argc, char** argv) {
    QGuiApplication application(argc, argv);
    QTemporaryDir temporary;
    auto& appStorage = storage::ApplicationStorage::instance();
    require(temporary.isValid() &&
                appStorage.initialize({temporary.path(), temporary.path(), 60000}).success,
            "isolated settings must initialize");
    timeDirectories(temporary);
    windowVariables(temporary);
    appStorage.shutdown();
    return EXIT_SUCCESS;
}
