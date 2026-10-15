# Youdao Ziyue Lite translation

Snow Shot supports the native Youdao Ziyue Lite text translation API, independently
of the existing Youdao NMT provider. No local proxy, model download, MCP server,
or additional background process is required.

In **Settings > Network and Connections > Translation Services**, add a
configuration with:

- **Service Format:** Youdao Ziyue Lite
- **API URL:** `https://openapi.youdao.com/proxy/http/llm-trans`
- **Application ID:** the ID of your Youdao application
- **Application Secret:** the corresponding application secret, not a Bearer API key
- **Concurrency:** start with 4

Enable the large-model text translation service for that application in the
Youdao console, then select the configuration in Snow Shot's translation model
selector. Credentials remain in the application's existing configuration storage;
do not include them in source code, screenshots, bug reports, or unredacted exports.

The endpoint is configurable, as with the other providers. Only use the official
HTTPS endpoint or a server you trust: requests contain the application ID, source
text, and a time-limited signature. Redirects remain restricted to the same origin.

The adapter selects `handleOption=3`, signs `i` using v3/SHA-256, and requests
`streamType=increment`. Translation increments are displayed as they arrive.
Youdao ends successful streams by closing the HTTP response; it does not use the
OpenAI `[DONE]` marker. This provider accepts up to 5000 Unicode code points per
request. Larger individual translation items fail locally rather than silently
truncating the source or sending an unsupported request.

Existing DeepL, Baidu, and Youdao NMT configurations retain their provider IDs
and request/response formats. Youdao NMT remains a separate service; replacing
its URL with the Lite URL does not switch its protocol.

Official API reference:
[Youdao large-model text translation](https://ai.youdao.com/DOCSIRMA/html/trans/api/dmxfy/index.html).

## Targeted tests

Build and run only the affected targets, using a configured preset with tests
enabled:

```powershell
cmake --build --preset build-windows-msvc-debug --target snow-shot-youdao-lite-protocol-tests snow-shot-text-translation-tests snow-shot-text-translation-settings-tests snow-shot-api-client-tests
ctest --preset test-windows-msvc-debug -R '^snow-shot-(youdao-lite-protocol|text-translation|text-translation-settings|api-client|api-client-lifetime)-tests$'
python snow_shot/tests/translation_catalog_tests.py
```

These deterministic tests use local fixture servers, not billable Youdao
requests. A live API smoke test must be explicitly configured with private
credentials; do not assume fixture success verifies a particular account's
service permissions, quota, or network connectivity.
