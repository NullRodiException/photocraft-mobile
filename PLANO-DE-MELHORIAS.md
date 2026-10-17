# Plano de melhorias

Levantamento de 16/10/2026, feito sobre o commit `01651d0` (APK v8) com o PhotoCraft em `db46713c`.

Cada item traz o problema, a proposta, onde mexer e como verificar.

- **Prioridade.** **P0**: risco de perder trabalho ou bloqueio de uso. **P1**: a experiência central de desenhar no tablet. **P2**: acabamento e qualidade. **P3**: publicação.
- **Esforço.** **P**: mudança local num arquivo. **M**: vários arquivos, ou Java e Rust juntos. **G**: recurso novo.

Uma regra vale para todos os itens: nada aqui altera o `../photocraft`. Quando a solução certa estiver no PhotoCraft, o item diz isso, e o caminho é abrir uma issue em storytold/photocraft.

## Resumo

✅ marca os itens já feitos.

| # | Item | Prior. | Esforço |
|---|---|---|---|
| 1 | Recuperação após falha ou encerramento pelo Android ✅ | P0 | M |
| 2 | Salvar um arquivo aberto pelo seletor não chega ao original ✅ | P0 | M |
| 3 | O texto do teclado na tela não chega aos campos ✅ | P0 | M |
| 4 | Fixar a versão do PhotoCraft usada ✅ | P0 | P |
| 5 | Amostras intermediárias da caneta | P1 | M |
| 6 | Caneta com outro dedo já na tela ✅ | P1 | P |
| 7 | Rejeição de palma e desfazer acidental | P1 | M |
| 8 | Gestos que faltam (girar, encaixar, tela cheia, conta-gotas) | P1 | M |
| 9 | Latência do traço | P1 | M–G |
| 10 | Cache de arquivos que só cresce ✅ | P1 | P |
| 11 | Arquivos recentes que sobrevivem ao cache | P1 | M |
| 12 | Abrir com, Compartilhar e Salvar na Galeria | P1 | M |
| 13 | Área de transferência de imagens e links | P2 | M |
| 14 | Layout para celular em retrato | P2 | M |
| 15 | Barra lateral à direita | P2 | P |
| 16 | Textos do shell só em inglês e português | P2 | M |
| 17 | Configurações que não são salvas | P2 | P |
| 18 | Painel de camadas pensado para toque | P2 | G |
| 19 | Reagir à falta de memória | P2 | P |
| 20 | Medir desempenho no aparelho | P2 | P |
| 21 | `cargo xtask ci` e CI no GitHub ✅ | P2 | M |
| 22 | Testes de snapshot da interface | P2 | M |
| 23 | Ponte Java ↔ Rust testável | P2 | P |
| 24 | Relatório de falhas | P2 | P |
| 25 | Documentação desatualizada | P2 | P |
| 26 | Assinatura de release e AAB | P3 | M |
| 27 | Requisitos atuais da Play Store | P3 | P |
| 28 | Escolher pasta no seletor | P3 | M |

---

## P0: não perder trabalho

### 1. Recuperação após falha ou encerramento pelo Android

- **Problema:** `services::base` (`crates/shell/src/services.rs`) não preenche os serviços de recuperação do PhotoCraft (`autosave`, `autosave_results`, `discard_autosave`, `recover`, `adopt_autosave`). O app de desktop do PhotoCraft liga esses serviços em `recovery_services` (`../photocraft/apps/photocraft/src/services.rs`). No Android, um app em segundo plano pode ser encerrado a qualquer momento para liberar memória. Hoje, nesse caso, perde-se tudo o que não foi salvo.
- **Proposta:**
  - Criar `recovery_services(dir)` no shell, como o do PhotoCraft, usando a API pública do `photocraft-format` (`RecoveryStore`, `list_recovery`, `recover`). A pasta fica em `recovery/` dentro do armazenamento interno (Android) ou da pasta de configurações (desktop).
  - Além do autosave periódico, salvar quando o app vai para segundo plano. `MainActivity.onPause` avisa o Rust por um novo `native…`, e o shell chama `prefs_ui::autosave_now`.
  - Antes, confirmar que o eframe ainda roda um frame depois do `onPause`. Se não rodar, disparar o autosave pelo evento de suspensão.
- **Onde:** `crates/shell/src/services.rs`, `apps/android/src/lib.rs`, `MainActivity.java`.
- **Verificar:** pintar algo sem salvar, mandar o app para segundo plano, rodar `adb shell am kill dev.photocraftmobile.app` e abrir de novo. O PhotoCraft deve oferecer a recuperação. No desktop, encerrar o processo pelo Gerenciador de Tarefas.

### 2. Salvar um arquivo aberto pelo seletor não chega ao original

- **Problema:** Abrir copia o documento para `cache/saf-open/<id>/`, e o PhotoCraft o abre de lá. Mas `files::after_write` só exporta caminhos dentro de `/saf-save/`, e a `MainActivity` só conhece os destinos criados por Salvar Como (`saveTargets`). Pelo código, então, **Salvar** num documento aberto grava só a cópia do cache, e o arquivo original não muda.
  - Há outro problema: `saveTargets` é um `HashMap` na memória da activity. Se o processo for recriado, "Salvar de novo" deixa de chegar ao documento escolhido.
  - Falta confirmar no aparelho: abrir um `.psd` ou `.pcraft`, editar, salvar e conferir o original no app Arquivos.
- **Proposta:**
  - Guardar a URI de cada arquivo aberto ao lado do caminho no cache, com `takePersistableUriPermission` de leitura e escrita. Persistir esse mapa num arquivo do armazenamento interno, e não só na memória.
  - `after_write` passa a exportar qualquer caminho que tenha uma URI associada.
  - Quando a URI não aceitar escrita, o app cai em Salvar Como.
- **Onde:** `MainActivity.java` (`onActivityResult`, `export`), `apps/android/src/files.rs`.
- **Verificar:** o roteiro acima, também depois de `adb shell am kill` entre o primeiro e o segundo salvamento.

### 3. O texto do teclado na tela não chega aos campos

- **Problema:** no emulador, tocar num campo abre o teclado, mas o texto digitado não aparece. Isso foi visto no v6 e no v8, e afeta a busca de Ações, renomear camadas, a ferramenta Texto e o diálogo Novo documento. Ainda não foi testado no Tab S9 FE.
- **Causa provável (confirmada no código):** com a GameActivity, o teclado entrega o texto como `InputEvent::TextEvent`, com o estado inteiro do campo. O winit 0.30.13 (`platform_impl/android/mod.rs`) só trata `MotionEvent` e `KeyEvent`, então o texto nunca vira `Ime::Commit` nem `Event::Text` no egui.
- **Proposta:** o crate Android guarda um clone do `AndroidApp`. A cada frame, `input_hook` lê `AndroidApp::text_input_state()`, compara com o estado anterior e gera os `Event::Text` e Backspace equivalentes. Quando o egui muda o foco, `set_text_input_state` mantém o campo nativo em sincronia, para que o teclado não "lembre" texto de outro campo. A diferença de estados é lógica pura, então pode ficar no shell, com testes.
- **Onde:** um `apps/android/src/ime.rs` novo, mais `apps/android/src/lib.rs` e `crates/shell/src/` para a parte testável.
- **Verificar:** em Ações › Buscar comandos, digitar com o teclado e com `adb shell input text camada`, incluindo acentos e apagar.

### 4. Fixar a versão do PhotoCraft usada

- **Problema:** a dependência por caminho segue o que estiver em `../photocraft`, e não há registro de qual commit foi testado. O pull de 16/10 trouxe 46 commits. Depois dele, `cargo check --workspace --all-targets` passou sem erros nem avisos e os 65 testes do `cargo test` passaram, mas qualquer pull futuro pode quebrar o build sem aviso. O APK não foi gerado de novo.
- **Proposta:**
  - Criar um arquivo `photocraft.rev` com o commit testado.
  - `cargo xtask ci` (item 21) compara esse commit com `git -C ../photocraft rev-parse HEAD` e avisa quando forem diferentes.
  - Acrescentar ao README a seção "Atualizar o PhotoCraft": pull, build, testes, snapshots (item 22), `frame_bench`, atualizar `photocraft.rev` e o `Cargo.lock`, e então commit.
- **Onde:** `xtask/src/main.rs`, `README.md`.

---

## P1: caneta e desenho

### 5. Amostras intermediárias da caneta

- **Problema:** o Android agrupa as amostras da caneta e entrega um `MotionEvent` por quadro, com as amostras anteriores no histórico (`getHistorySize`, `getHistoricalX/Y/Pressure/AxisValue`). Duas coisas se perdem:
  - `MainActivity.reportPen` manda só a amostra atual. O `StylusFeed` do PhotoCraft já enfileira todas as amostras e interpola a pressão de cada movimento, então a pressão e a inclinação perdem resolução.
  - O winit 0.30.13 também usa só a posição atual de cada ponteiro, o que foi confirmado no código. Com a caneta rápida, curvas viram segmentos retos.
- **Proposta:**
  1. Em `reportPen`, mandar as amostras do histórico em ordem antes da atual.
  2. Em `pen::input_hook`, injetar as posições históricas como `Event::Touch { phase: Move }` extras, antes do evento que o winit gerou para o mesmo ponteiro. O Java guarda essas posições e o Rust as busca uma vez por frame, como já faz com o hover.
- **Onde:** `MainActivity.java`, `apps/android/src/pen.rs`.
- **Verificar:** desenhar círculos rápidos com um pincel fino e comparar capturas de tela de antes e depois. Registrar no log quantas amostras históricas chegam por evento.

### 6. Caneta com outro dedo já na tela

- **Problema:** `isPen` e `reportPen` olham só o ponteiro de `getActionIndex()`, que em `ACTION_MOVE` é sempre o 0. Se a mão tocou a tela antes de a caneta entrar no alcance, ela não é rejeitada e vira o ponteiro 0. Aí os movimentos da caneta não chegam ao `nativePenSample`, e a pressão fica parada no valor do primeiro toque.
- **Proposta:** percorrer todos os ponteiros do evento, reportar os que forem `TOOL_TYPE_STYLUS` ou `TOOL_TYPE_ERASER` e decidir a rejeição de palma por ponteiro.
- **Onde:** `MainActivity.java`.
- **Verificar:** com a caneta longe, apoiar a mão na tela e depois desenhar variando a pressão.

### 7. Rejeição de palma e desfazer acidental

- **Problema:** a rejeição só vale enquanto a caneta está no alcance e por 0,5 s depois. Com a caneta longe, uma palma que encosta em dois pontos e sai rápido conta como toque com dois dedos e desfaz algo sem querer. O Android 13+ marca com `FLAG_CANCELED` os contatos que o sistema reconheceu como palma, e hoje isso é ignorado.
- **Proposta:**
  - A `MainActivity` avisa o Rust quando um ponteiro foi cancelado como palma. O `TouchRouter` não conta toque (nem desfazer ou refazer) num gesto que teve contato cancelado.
  - Descartar contatos muito largos (`getTouchMajor()`).
  - Tornar o tempo de 0,5 s configurável.
- **Onde:** `MainActivity.java`, `apps/android/src/touch_tools.rs`, `crates/shell/src/touch.rs`.
- **Verificar:** um teste em `touch.rs` em que um contato cancelado não gera `tap`, além de teste manual com a mão.

### 8. Gestos que faltam (paridade com o Procreate)

| Gesto | Ação | Base que já existe |
|---|---|---|
| girar com 2 dedos | gira a vista, encaixando em 0°/90°/180°/270° quando perto | `pan_view` já considera `view.rotation` |
| pinça rápida e solta | Encaixar na tela | `view.fitOnScreen` (`Act::Fit`) |
| toque com 4 dedos | tela cheia (só a imagem) | `toggle_focus` |
| segurar 1 dedo parado (~0,5 s) | conta-gotas; soltar volta à ferramenta | `Act::Modify` |

- **Onde:** `crates/shell/src/touch.rs` (a `Gesture` ganha `rotate` e `hold`) e `apply_gesture` em `crates/shell/src/lib.rs`. Testes no estilo dos que já existem em `touch.rs`.

### 9. Latência do traço

- **Problema:** nada mede a latência entre a ponta da caneta e o traço. O `frame_bench` mede só CPU.
- **Proposta,** do mais barato ao mais caro:
  1. Chamar `View.requestUnbufferedDispatch` no DOWN da caneta, para as amostras chegarem assim que existem, sem esperar o quadro. Isso gera mais eventos, então o custo de CPU deve ser medido junto.
  2. Testar `desired_maximum_frame_latency = 1` e o present mode nas `wgpu_options` do eframe (`apps/android/src/lib.rs`).
  3. Predição de movimento (`androidx.input:input-motionprediction`). Isso exige que o PhotoCraft desenhe um trecho provisório do traço, então começa por uma issue no upstream.
- **Verificar:** filmar em câmera lenta a ponta da caneta e o traço, antes e depois de cada passo.

---

## P1: arquivos e integração com o Android

### 10. Cache de arquivos que só cresce

- **Problema:** as pastas `cache/saf-open/<id>` e `cache/saf-save/<id>` nunca são apagadas. Cada foto aberta fica copiada no cache.
- **Proposta:** ao iniciar, apagar as pastas que nenhum documento aberto ou recente usa. Ao fechar um documento sem alterações, apagar a cópia dele.
- **Onde:** `MainActivity.java`, ou o Rust no início do `android_main`.

### 11. Arquivos recentes que sobrevivem ao cache

- **Problema:** os recentes do PhotoCraft guardam o caminho no cache. Quando o item 10 ou o próprio Android limpar o cache, os recentes passam a falhar.
- **Proposta:** com as URIs persistidas do item 2, ao iniciar, a `MainActivity` recopia para o cache os recentes que sumiram, antes de a tela inicial aparecer.
- **Depende de:** item 2.

### 12. Abrir com, Compartilhar e Salvar na Galeria

- **Proposta:**
  - **Entrada:** filtros `ACTION_VIEW` e `ACTION_SEND` para `image/*` (e `.psd`/`.pcraft`) no manifesto, para que "Abrir com" e "Compartilhar → PhotoCraft Mobile" funcionem. O arquivo recebido é copiado para o cache como no Abrir, e uma fila nova leva o caminho ao shell, que chama `app.open_path`.
  - **Saída:** em Ações › Rápido, adicionar **Compartilhar…** (`ACTION_SEND` com `FileProvider`) e **Salvar na Galeria** (`MediaStore`, sem abrir o seletor).
- **Onde:** `AndroidManifest.xml`, `MainActivity.java`, `apps/android/src/files.rs`, `quick_rows` em `studio.rs`.

### 13. Área de transferência de imagens e links

- **Problema:**
  - `services::base` não preenche `clipboard_set_image` nem `clipboard_get_image`, então Copiar e Colar imagens só funciona dentro do app.
  - `open_url` também não é preenchido, e o PhotoCraft cai em `ctx.open_url`. Ainda não foi verificado se isso abre o navegador no Android, por exemplo no botão Discord da tela inicial ou nos links de Ajuda.
- **Proposta:**
  - Clipboard: usar `ClipboardManager` com uma URI de conteúdo (`FileProvider`), passando pela mesma fila de pedidos do `files.rs`.
  - Links: se não abrirem, implementar `open_url` com um `Intent.ACTION_VIEW`.

---

## P2: interface

### 14. Layout para celular em retrato

- **Problema:** a barra superior tem uns 15 controles: Galeria, 3 popovers, Transformar, o seletor de modos, Todas as ferramentas, 3 ferramentas rápidas, Camadas e Cor. Abaixo de 760 pt, `COMPACT_BELOW` só tira os rótulos. Num celular em retrato (~360–430 pt de largura), a barra provavelmente não cabe.
- **Proposta:**
  1. Conferir com o `snapshot` num tamanho de celular.
  2. Criar um layout "celular": o seletor de modos vira um botão só, as ferramentas rápidas, desfazer e refazer descem para uma barra inferior, e a barra lateral fica mais curta.
- **Onde:** `crates/shell/src/studio.rs` (`top_bar`, `side_bar`).

### 15. Barra lateral à direita

- **Proposta:** uma opção em Ações › Rápido para pôr a barra lateral à direita (para canhotos, como no Procreate). Ela é salva em `ShellSettings`, e o popover de Sensibilidade, que hoje abre ao lado dela, passa a abrir do outro lado.
- **Onde:** `side_bar` e `popover` em `studio.rs`, `ShellSettings`.

### 16. Textos do shell só em inglês e português

- **Problema:** `tr(en, pt)` em `studio.rs` e `text(en, pt)` na `MainActivity` cobrem só dois idiomas. O PhotoCraft tem mais de 20 (`i18n/*.tsv`), e `Mode::label` já trata o espanhol.
- **Proposta:**
  - Criar uma tabela própria em `crates/shell/src/i18n/<idioma>.tsv`, no mesmo formato do PhotoCraft: a chave é o texto em inglês, e o inglês é o fallback.
  - No Java, usar `res/values-<idioma>/strings.xml`.
  - Começar pelo espanhol.

### 17. Configurações que não são salvas

- As cores recentes (`Studio::recent_colors`) se perdem ao fechar o app. Elas devem ir para `ShellSettings`.
- No desktop, `apps/desktop/src/main.rs` não chama `with_settings`. Por isso o seletor de cor e "Sempre mostrar a barra de opções" voltam ao padrão a cada execução.

### 18. Painel de camadas pensado para toque

- **Problema:** Camadas abre o painel do dock do PhotoCraft, que tem densidade de desktop e só muda alterando o PhotoCraft.
- **Proposta:** um popover de camadas do próprio shell, como o do Procreate: miniaturas grandes, visibilidade, opacidade, modo de mesclagem e arrastar para reordenar, tudo por comandos públicos do engine. Antes de começar, levantar quais comandos `layer.*` existem e como obter as miniaturas. O painel do PhotoCraft continua acessível por um "Mais…".

---

## P2: desempenho e memória

### 19. Reagir à falta de memória

- **Problema:** a `MainActivity` ignora `onTrimMemory`. Os limites de memória só são definidos na primeira execução.
- **Proposta:** em `TRIM_MEMORY_UI_HIDDEN` e `TRIM_MEMORY_BACKGROUND`, avisar o Rust. O shell faz o autosave (item 1) e roda `edit.purge.videoCache` e `edit.purge.clipboard`. O histórico de desfazer nunca é apagado sem perguntar.

### 20. Medir desempenho no aparelho

- **Problema:** o `frame_bench` mede a CPU no desktop, mas nada mede o tablet.
- **Proposta:** uma opção de diagnóstico (em Ações › Rápido, ou por variável de ambiente) que a cada 5 s registra no logcat o tempo médio e o p95 dos frames e a memória do processo (`VmRSS` de `/proc/self/status`). Isso permite comparar builds no Tab S9 FE, inclusive ao abrir uma foto de 50 MP.

---

## P2: qualidade

### 21. `cargo xtask ci` e CI no GitHub

- **Problema:** não há um comando único de verificação nem CI, embora o repositório já esteja em `NullRodiException/photocraft-mobile`. O crate Android só compila para Android, então hoje só um build de APK o verifica.
- **Proposta:**
  - `cargo xtask ci` roda em sequência: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo ndk -t arm64-v8a check -p photocraft-mobile-android` e a checagem do `photocraft.rev` (item 4).
  - Um workflow do GitHub Actions faz o checkout deste repositório e do `storytold/photocraft` no commit de `photocraft.rev`, lado a lado, e roda o mesmo `xtask ci`. Antes, conferir se algum crate do PhotoCraft exige outro checkout irmão.

### 22. Testes de snapshot da interface

- **Problema:** `examples/snapshot.rs` gera PNGs, mas nada os compara. O `egui_kittest` com a feature `snapshot` já é dependência de desenvolvimento.
- **Proposta:** testes de snapshot para cada modo e cada popover em três tamanhos: tablet deitado, tablet em pé e celular. Isso pega regressões de layout depois de atualizar o PhotoCraft (item 4) e mostra o problema do item 14.

### 23. Ponte Java ↔ Rust testável

- **Problema:** o protocolo de linhas do `files.rs` e a decisão de `pen::sample` (ação → amostra no feed) ficam no crate Android, que não compila no desktop, e por isso não têm testes.
- **Proposta:** mover as partes puras (montar e ler as linhas, `clean`, o mapeamento de ações) para `crates/shell`, como já foi feito com `pen::tilt_from_android`, e testá-las lá.

### 24. Relatório de falhas

- **Proposta:** instalar um `std::panic::set_hook` que grava a mensagem e o backtrace em `files/crash.log`, além do logcat. Na execução seguinte, o app oferece compartilhar o arquivo. O nosso código não usa `panic!`, mas as dependências podem entrar em pânico, e hoje só se descobre isso com o cabo USB conectado.

### 25. Documentação desatualizada

- `apps/android/Cargo.toml`: a descrição e os comentários falam em `NativeActivity`, mas o app usa a `GameActivity`.

---

## P3: publicação

### 26. Assinatura de release e AAB

- **Problema:** o APK de release é assinado com a chave de debug (`build.gradle.kts`).
- **Proposta:**
  - Criar uma keystore de release fora do repositório, com a senha em variável de ambiente ou em `~/.gradle/gradle.properties`.
  - `cargo xtask android --release` gera um AAB (`bundleRelease`) para a Play Store. O build local continua com a chave de debug.
  - Atenção: um APK assinado com outra chave não instala por cima do atual. É preciso desinstalar uma vez.

### 27. Requisitos atuais da Play Store

- **Páginas de 16 KB:** a Play exige que apps que miram o Android 15+ suportem páginas de 16 KB. O NDK r28 já alinha por padrão, mas é preciso conferir o APK com `zipalign -c -P 16 -v 4 <apk>` e os segmentos `LOAD` da `libphotocraft_mobile.so` com `llvm-readelf -l` (alinhamento `0x4000`).
- **targetSdk:** hoje é 35. Conferir o nível que a Play exige na época da publicação, porque ele sobe todo ano.

### 28. Escolher pasta no seletor

- Esse item já está no README como pendência. A proposta é usar `ACTION_OPEN_DOCUMENT_TREE` com permissão persistida, ligada ao `FileDialogRequest::Folder` que o `files.rs` hoje recusa.

---

## Ordem sugerida

1. **Base segura:** 4 → 21 → 1 → 2 → 3 → 10. Com isso, o build fica reproduzível e verificado, e o trabalho do usuário não se perde.
2. **Desenho:** 6 → 5 → 7 → 24 → 8 → 9.
3. **Integração:** 12 → 11 → 13 → 17 → 19.
4. **Interface:** 22 → 14 → 15 → 16 → 18 → 20.
5. **Publicação:** 25 → 26 → 27 → 28.

## Depende do PhotoCraft (abrir issue no upstream)

- A densidade dos menus e diálogos para toque (já citada no README).
- O traço provisório para a predição de movimento (item 9).
- `native_menu::state_hash` público, para o shell não precisar copiar a lista de estados em `menu_state` (`studio.rs`).
