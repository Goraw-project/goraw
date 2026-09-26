const vscode = require('vscode');
const cp = require('child_process');
const path = require('path');
const fs = require('fs');

let lspProcess = null;
let diagnosticCollection = null;
let nextRequestId = 1;
const pendingRequests = new Map();
let incomingBuffer = Buffer.alloc(0);

function findGorawBinary() {
    const configPath = vscode.workspace.getConfiguration('goraw').get('lspPath');
    if (configPath && fs.existsSync(configPath)) {
        return configPath;
    }

    const candidates = [
        'P:\\Goraw\\target\\release\\goraw.exe',
        'P:\\Goraw\\target\\debug\\goraw.exe',
        'C:\\Users\\Letalty\\.cargo\\bin\\goraw.exe'
    ];

    for (const c of candidates) {
        if (fs.existsSync(c)) {
            return c;
        }
    }

    return 'goraw';
}

function startLspServer(outputChannel) {
    const binPath = findGorawBinary();
    outputChannel.appendLine(`[Goraw LSP] Запуск сервера: ${binPath} lsp`);

    try {
        lspProcess = cp.spawn(binPath, ['lsp'], {
            stdio: ['pipe', 'pipe', 'pipe']
        });
    } catch (err) {
        outputChannel.appendLine(`[Goraw LSP] Не удалось запустить ${binPath}: ${err.message}`);
        return;
    }

    lspProcess.on('error', (err) => {
        outputChannel.appendLine(`[Goraw LSP] Ошибка процесса: ${err.message}`);
    });

    lspProcess.stderr.on('data', (chunk) => {
        outputChannel.append(`[Goraw LSP stderr] ${chunk.toString('utf8')}`);
    });

    lspProcess.stdout.on('data', (chunk) => {
        incomingBuffer = Buffer.concat([incomingBuffer, chunk]);
        parseIncomingMessages(outputChannel);
    });

    lspProcess.on('exit', (code, signal) => {
        outputChannel.appendLine(`[Goraw LSP] Процесс завершился с кодом ${code}, сигнал ${signal}`);
        lspProcess = null;
    });

    // Отправляем initialize
    sendRequest('initialize', {
        capabilities: {}
    }).then(() => {
        sendNotification('initialized', {});
        outputChannel.appendLine('[Goraw LSP] Сервер успешно инициализирован.');

        // Синхронизируем все открытые документы
        for (const doc of vscode.workspace.textDocuments) {
            if (doc.languageId === 'goraw') {
                syncDocOpen(doc);
            }
        }
    }).catch(err => {
        outputChannel.appendLine(`[Goraw LSP] Ошибка initialize: ${err.message}`);
    });
}

function parseIncomingMessages(outputChannel) {
    while (true) {
        const headerEnd = incomingBuffer.indexOf('\r\n\r\n');
        if (headerEnd === -1) {
            break;
        }

        const headerStr = incomingBuffer.slice(0, headerEnd).toString('utf8');
        let contentLength = null;
        for (const line of headerStr.split('\r\n')) {
            if (line.toLowerCase().startsWith('content-length:')) {
                contentLength = parseInt(line.split(':')[1].trim(), 10);
            }
        }

        if (contentLength === null) {
            incomingBuffer = incomingBuffer.slice(headerEnd + 4);
            continue;
        }

        const bodyStart = headerEnd + 4;
        if (incomingBuffer.length < bodyStart + contentLength) {
            // Ждём оставшиеся байты
            break;
        }

        const bodyBuf = incomingBuffer.slice(bodyStart, bodyStart + contentLength);
        incomingBuffer = incomingBuffer.slice(bodyStart + contentLength);

        try {
            const msg = JSON.parse(bodyBuf.toString('utf8'));
            handleMessage(msg, outputChannel);
        } catch (err) {
            outputChannel.appendLine(`[Goraw LSP] Ошибка разбора JSON: ${err.message}`);
        }
    }
}

function handleMessage(msg, outputChannel) {
    if (msg.id !== undefined && msg.id !== null) {
        // Ответ на запрос
        const id = msg.id;
        if (pendingRequests.has(id)) {
            const { resolve, reject } = pendingRequests.get(id);
            pendingRequests.delete(id);
            if (msg.error) {
                reject(new Error(msg.error.message || 'Ошибка LSP'));
            } else {
                resolve(msg.result);
            }
        }
    } else if (msg.method) {
        // Уведомление
        if (msg.method === 'textDocument/publishDiagnostics') {
            const params = msg.params;
            if (params && params.uri) {
                const uri = vscode.Uri.parse(params.uri);
                const vsDiags = (params.diagnostics || []).map(d => {
                    const range = new vscode.Range(
                        d.range.start.line,
                        d.range.start.character,
                        d.range.end.line,
                        d.range.end.character
                    );
                    const severity = d.severity === 2
                        ? vscode.DiagnosticSeverity.Warning
                        : vscode.DiagnosticSeverity.Error;
                    const diag = new vscode.Diagnostic(range, d.message, severity);
                    diag.code = d.code;
                    diag.source = d.source || 'goraw';
                    return diag;
                });
                diagnosticCollection.set(uri, vsDiags);
            }
        }
    }
}

function sendRequest(method, params) {
    return new Promise((resolve, reject) => {
        if (!lspProcess || !lspProcess.stdin.writable) {
            return reject(new Error('LSP процесс не запущен'));
        }
        const id = nextRequestId++;
        pendingRequests.set(id, { resolve, reject });

        const req = {
            jsonrpc: '2.0',
            id,
            method,
            params
        };
        const body = JSON.stringify(req);
        const msg = `Content-Length: ${Buffer.byteLength(body, 'utf8')}\r\n\r\n${body}`;
        lspProcess.stdin.write(msg, 'utf8');
    });
}

function sendNotification(method, params) {
    if (!lspProcess || !lspProcess.stdin.writable) {
        return;
    }
    const notif = {
        jsonrpc: '2.0',
        method,
        params
    };
    const body = JSON.stringify(notif);
    const msg = `Content-Length: ${Buffer.byteLength(body, 'utf8')}\r\n\r\n${body}`;
    lspProcess.stdin.write(msg, 'utf8');
}

function syncDocOpen(document) {
    sendNotification('textDocument/didOpen', {
        textDocument: {
            uri: document.uri.toString(),
            languageId: 'goraw',
            version: document.version,
            text: document.getText()
        }
    });
}

function syncDocChange(document) {
    sendNotification('textDocument/didChange', {
        textDocument: {
            uri: document.uri.toString(),
            version: document.version
        },
        contentChanges: [{
            text: document.getText()
        }]
    });
}

function activate(context) {
    const outputChannel = vscode.window.createOutputChannel('Goraw Language Server');
    context.subscriptions.push(outputChannel);

    diagnosticCollection = vscode.languages.createDiagnosticCollection('goraw');
    context.subscriptions.push(diagnosticCollection);

    startLspServer(outputChannel);

    // Документы
    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument(doc => {
            if (doc.languageId === 'goraw') {
                syncDocOpen(doc);
            }
        }),
        vscode.workspace.onDidChangeTextDocument(e => {
            if (e.document.languageId === 'goraw') {
                syncDocChange(e.document);
            }
        }),
        vscode.workspace.onDidCloseTextDocument(doc => {
            if (doc.languageId === 'goraw') {
                diagnosticCollection.delete(doc.uri);
                sendNotification('textDocument/didClose', {
                    textDocument: { uri: doc.uri.toString() }
                });
            }
        })
    );

    // Hover
    context.subscriptions.push(
        vscode.languages.registerHoverProvider('goraw', {
            async provideHover(document, position) {
                try {
                    const res = await sendRequest('textDocument/hover', {
                        textDocument: { uri: document.uri.toString() },
                        position: { line: position.line, character: position.character }
                    });
                    if (res && res.contents) {
                        return new vscode.Hover(new vscode.MarkdownString(res.contents.value));
                    }
                } catch (e) {
                    // игнорируем ошибку запроса
                }
                return null;
            }
        })
    );

    // Definition (F12)
    context.subscriptions.push(
        vscode.languages.registerDefinitionProvider('goraw', {
            async provideDefinition(document, position) {
                try {
                    const res = await sendRequest('textDocument/definition', {
                        textDocument: { uri: document.uri.toString() },
                        position: { line: position.line, character: position.character }
                    });
                    if (res && res.uri && res.range) {
                        const targetUri = vscode.Uri.parse(res.uri);
                        const targetRange = new vscode.Range(
                            res.range.start.line,
                            res.range.start.character,
                            res.range.end.line,
                            res.range.end.character
                        );
                        return new vscode.Location(targetUri, targetRange);
                    }
                } catch (e) {
                    // игнорируем
                }
                return null;
            }
        })
    );

    // Completion (автодополнение на '.' и '::')
    context.subscriptions.push(
        vscode.languages.registerCompletionItemProvider('goraw', {
            async provideCompletionItems(document, position) {
                try {
                    const items = await sendRequest('textDocument/completion', {
                        textDocument: { uri: document.uri.toString() },
                        position: { line: position.line, character: position.character }
                    });
                    if (Array.isArray(items)) {
                        return items.map(item => {
                            const vsItem = new vscode.CompletionItem(item.label);
                            vsItem.kind = item.kind;
                            vsItem.detail = item.detail;
                            if (item.documentation) {
                                vsItem.documentation = new vscode.MarkdownString(item.documentation.value);
                            }
                            if (item.insert_text) {
                                vsItem.insertText = new vscode.SnippetString(item.insert_text);
                            }
                            return vsItem;
                        });
                    }
                } catch (e) {
                    // игнорируем
                }
                return [];
            }
        }, '.', ':')
    );

    outputChannel.appendLine('[Goraw LSP] Расширение активировано, провайдеры зарегистрированы.');
}

function deactivate() {
    if (lspProcess) {
        try {
            sendRequest('shutdown', {}).finally(() => {
                sendNotification('exit', {});
                lspProcess.kill();
                lspProcess = null;
            });
        } catch (_) {
            lspProcess.kill();
            lspProcess = null;
        }
    }
}

module.exports = {
    activate,
    deactivate
};
