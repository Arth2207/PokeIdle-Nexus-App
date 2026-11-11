# 0008. Imagens compiladas só no CI

- Status: aceito
- Data: 2026-11-11

## Contexto

Compilar Rust em release com LTO ocupa todos os núcleos por vários minutos. A VPS tem 2 vCPU e roda produção. Compilar nela significa degradar o serviço durante todo deploy.

## Decisão

- As imagens são compiladas só no GitHub Actions e publicadas no GitHub Container Registry.
- A VPS só faz `docker compose pull` e a troca blue/green.
- O desenvolvimento local acontece na máquina do desenvolvedor, com `rustup`.
- As queries do sqlx são validadas no desenvolvimento e versionadas em `.sqlx/`. Por isso o CI compila com `SQLX_OFFLINE=true`, sem precisar de banco.

## Consequências

- Deploy sem impacto de CPU na produção.
- Todas as imagens são reproduzíveis, porque o build parte de um ambiente limpo.
- O cache de dependências (`cargo-chef` e o cache do Actions) mantém o build incremental em poucos minutos.
- Custo: não dá para fazer hotfix compilando direto no servidor. Isso é intencional: toda mudança passa pelo pipeline e pelos testes.
