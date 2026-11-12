-- Fase 3: ingestão de eventos, configuração de hunt, fila de comandos, estado de raros e
-- chave de idempotência das tentativas de boss. Só aditiva, e só no schema `nexus`.

-- Eventos brutos do jogo (kills, capturas, drops...). A chave é um hash do conteúdo, então
-- reenviar o mesmo lote não duplica linha. O Timescale exige a coluna de partição (`em`) em
-- todo índice único.
CREATE TABLE nexus.evento (
    conta       text        NOT NULL,
    -- `conta_id` como veio no lote (normalizado), quando há nick e os dois diferem.
    conta_id    text,
    tipo        text        NOT NULL,
    em          timestamptz NOT NULL,
    chave       bytea       NOT NULL,
    dados       jsonb       NOT NULL,
    recebido_em timestamptz NOT NULL DEFAULT now(),
    UNIQUE (chave, em)
);
CREATE INDEX evento_conta_tipo_em ON nexus.evento (conta, tipo, em DESC);

-- Hypertable de chunks de 1 dia, quando a extensão existe (sem ela, fica tabela comum).
-- Compressão e retenção ficam para depois.
-- A função é qualificada pelo schema da extensão: o search_path do role do Nexus
-- é só `nexus`, e em produção a extensão mora em `public`.
DO $$
DECLARE
    esquema text;
BEGIN
    SELECT extnamespace::regnamespace::text INTO esquema
      FROM pg_extension WHERE extname = 'timescaledb';
    IF esquema IS NOT NULL THEN
        EXECUTE format(
            'SELECT %I.create_hypertable(%L::regclass, %L::name, chunk_time_interval => interval %L)',
            esquema, 'nexus.evento', 'em', '1 day');
    END IF;
END
$$;

-- Hunt atual de cada conta (`POST /api/hunt-sync`).
CREATE TABLE nexus.config_hunt (
    conta         text PRIMARY KEY,
    dados         jsonb       NOT NULL,
    atualizado_em timestamptz NOT NULL DEFAULT now()
);

-- Fila de comandos. O estado é derivado: pendente = sem `executado_em` e sem `reservado_em`;
-- reservado = com `reservado_em`; encerrado = com `executado_em`.
CREATE TABLE nexus.comando (
    id            bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    conta         text        NOT NULL,
    tipo          text        NOT NULL,
    params        jsonb       NOT NULL DEFAULT '{}',
    prioridade    int         NOT NULL DEFAULT 5,
    origem        text        NOT NULL DEFAULT 'manual',
    criado_em     timestamptz NOT NULL DEFAULT now(),
    expira_em     timestamptz,
    reservado_em  timestamptz,
    reservado_por text,
    tentativas    int         NOT NULL DEFAULT 0,
    executado_em  timestamptz,
    resultado     text,
    -- Id da linha de `public.comando_fila` quando o comando é um espelho dela.
    id_legado     bigint      UNIQUE
);
CREATE INDEX comando_pendente ON nexus.comando (conta, prioridade, id) WHERE executado_em IS NULL;

-- Avisa o `nexus-api` (LISTEN nexus_comando) de cada mudança, mesmo quando quem escreve é
-- outro processo.
CREATE FUNCTION nexus.comando_notificar() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    evento text;
BEGIN
    -- Só avisa o que alguém consome: comando novo ainda pendente, devolvido à fila ou expirado.
    -- Reserva e execução não geram aviso.
    IF TG_OP = 'INSERT' THEN
        IF NEW.executado_em IS NOT NULL THEN
            RETURN NULL;
        END IF;
        evento := 'novo';
    ELSIF NEW.executado_em IS NOT NULL AND OLD.executado_em IS NULL THEN
        IF NEW.resultado IS DISTINCT FROM 'expirado' THEN
            RETURN NULL;
        END IF;
        evento := 'expirado';
    ELSIF NEW.reservado_em IS NULL AND OLD.reservado_em IS NOT NULL AND NEW.executado_em IS NULL THEN
        evento := 'devolvido';
    ELSE
        RETURN NULL;
    END IF;
    PERFORM pg_notify('nexus_comando',
        json_build_object('id', NEW.id, 'conta', NEW.conta, 'evento', evento)::text);
    RETURN NULL;
END
$$;

CREATE TRIGGER comando_notificar AFTER INSERT OR UPDATE ON nexus.comando
    FOR EACH ROW EXECUTE FUNCTION nexus.comando_notificar();

-- Estado de raros e shiny: uma linha `global` com o mesmo objeto do `raros-estado.json`
-- (`raros`, `em`, `hist`, `histEm`, `acc`).
CREATE TABLE nexus.estado_raros (
    chave         text PRIMARY KEY CHECK (chave = 'global'),
    estado        jsonb       NOT NULL,
    atualizado_em timestamptz NOT NULL DEFAULT now()
);

-- Idempotência das tentativas de boss: (conta, boss, instante de início) viram uma chave.
ALTER TABLE nexus.boss_tentativa ADD COLUMN chave text;
CREATE UNIQUE INDEX boss_tentativa_chave ON nexus.boss_tentativa (chave) WHERE chave IS NOT NULL;
