#!/usr/bin/env python3
"""Valida Top Catch em PostgreSQL 16 descartável, sem TCP nem banco de produção.
Requer root e o usuário postgres: python3 ferramentas/fixtures/testar-capturas.py
"""
import json
import pathlib
import shutil
import subprocess
import tempfile

base = pathlib.Path(tempfile.mkdtemp(prefix="nexus-capturas-teste-"))
subprocess.run(["chown", "postgres:postgres", str(base)], check=True)
pg_bin = "/usr/lib/postgresql/16/bin/"


def pg(*args):
    return subprocess.run(
        ["runuser", "-u", "postgres", "--", *args],
        check=True, capture_output=True, text=True,
    )


def sql(comando):
    return pg(
        pg_bin + "psql", "-h", str(base), "-p", "55439", "-d", "postgres",
        "-At", "-v", "ON_ERROR_STOP=1", "-c", comando,
    ).stdout.strip()


try:
    pg(pg_bin + "initdb", "-D", str(base / "data"), "-A", "trust", "--no-locale", "--encoding=UTF8")
    pg(pg_bin + "pg_ctl", "-D", str(base / "data"), "-l", str(base / "log"),
       "-o", f"-k {base} -p 55439 -c listen_addresses=''", "-w", "start")
    sql("""
        CREATE SCHEMA nexus;
        CREATE TABLE nexus.evento(conta text, conta_id text, tipo text, em timestamptz, chave bytea, dados jsonb);
        CREATE TABLE nexus.motor_janela(conta text, janela jsonb);
        CREATE TABLE nexus.pokedex_motor(id bigint PRIMARY KEY, dados jsonb);
        INSERT INTO nexus.pokedex_motor VALUES
          (63,'{"n":"Abra","c":"pokebola"}'), (16,'{"n":"Pidgey","c":"pokebola"}'),
          (25,'{"n":"Pikachu","c":"pokebola"}'), (19,'{"n":"Rattata","c":"pokebola"}'),
          (133,'{"n":"Eevee","c":"pokebola"}'), (29,'{"n":"Nidoran♀","c":"pokebola"}'),
          (32,'{"n":"Nidoran♂","c":"pokebola"}'), (151,'{"n":"Mew","c":"arena_mistica"}'),
          (1000,'{"n":"Mega Abra","c":"mega"}'), (2001,'{"n":"Brave Blastoise","c":"pokebola"}');
    """)
    consulta = (pathlib.Path(__file__).resolve().parents[2] /
                "crates/persistencia/src/sql/capturas_semana.sql").read_text()
    agora = sql("SELECT (extract(epoch FROM '2026-11-18 12:00:00-03'::timestamptz)*1000)::bigint")

    def query(instante=None):
        comando = ("PREPARE capturas(text,bigint) AS " + consulta +
                   f"; EXECUTE capturas('conta-a',{instante or agora});")
        return json.loads(sql(comando).splitlines()[-1])

    v = query()
    assert v["total"] == 0 and v["top_catch"]["projecao"] is None and len(v["dias"]) == 7, v
    sql("""INSERT INTO nexus.motor_janela VALUES ('conta-a','{"nick":"Jogador"}');
        INSERT INTO nexus.evento VALUES
        ('jogador','conta-a','welcome','2026-11-15 00:00:00-03',decode('01','hex'),'{}'),
        ('jogador','conta-a','capturado','2026-11-18 11:00:00-03',decode('02','hex'),'{"id":1,"especie":"Abra","nivel":5}'),
        ('jogador','conta-a','capturado','2026-11-18 11:00:01-03',decode('03','hex'),'{"seq":2,"especie":"Abra","nivel":5}'),
        ('jogador','conta-a','capturado','2026-11-18 11:00:02-03',decode('04','hex'),'{"id":1,"especie":"Abra","nivel":5}'),
        ('jogador','conta-a','capturado','2026-11-18 11:30:00-03',decode('05','hex'),'{"seq":3,"especie":"Pidgey","nivel":10}'),
        ('jogador','conta-a','capturado','2026-11-15 00:00:00-03',decode('06','hex'),'{"id":2,"especie":"Pikachu","nivel":20}'),
        ('jogador','conta-a','capturado','2026-11-14 23:59:59-03',decode('07','hex'),'{"id":3,"especie":"Mew"}'),
        ('outra',null,'capturado','2026-11-18 11:00:00-03',decode('08','hex'),'{"id":4,"especie":"Mew"}');""")
    v = query()
    tc = v["top_catch"]
    assert v["total"] == 3 and tc["especies"] == 3 and tc["recentes"] == 2, v
    assert v["horas_observadas"] == 24 and abs(tc["por_hora"] - 2 / 24) < 1e-9, v
    assert abs(v["projecao"] - 10) < 1e-9 and tc["projecao"] == 7, v
    assert [d["total"] for d in tc["dias"]] == [1, 0, 0, 2, 0, 0, 0], v
    assert tc["capturadas_ids"] == [16, 25, 63], v
    # Outra captura de Abra não aumenta espécies nem o ritmo de novas.
    sql("""INSERT INTO nexus.evento VALUES
        ('jogador','conta-a','capturado','2026-11-18 11:40:00-03',decode('09','hex'),'{"id":5,"especie":"Abra"}'),
        ('jogador','conta-a','capturado','2026-11-18 11:41:00-03',decode('0a','hex'),'{"id":6,"especie":"Mew"}'),
        ('jogador','conta-a','capturado','2026-11-18 11:42:00-03',decode('0b','hex'),'{"id":7,"especie":"Mega Abra"}'),
        ('jogador','conta-a','capturado','2026-11-18 11:43:00-03',decode('0c','hex'),'{"id":8,"especie":"Brave Blastoise"}'),
        ('jogador','conta-a','capturado','2026-11-18 11:44:00-03',decode('0d','hex'),'{"id":9,"especie":"Rattata","bola":"beast"}'),
        ('jogador','conta-a','capturado','2026-11-18 11:45:00-03',decode('0e','hex'),'{"id":10,"especie":"Desconhecido"}'),
        ('jogador','conta-a','capturado','2026-11-18 11:46:00-03',decode('0f','hex'),'{"id":11,"especie":"Apelido","speciesId":29}'),
        ('jogador','conta-a','capturado','2026-11-18 11:47:00-03',decode('10','hex'),'{"id":12,"especie":"Nidoran♂"}');""")
    v = query()
    tc = v["top_catch"]
    assert v["total"] == 11 and tc["especies"] == 5 and tc["recentes"] == 4, v
    assert tc["descartadas"] == 4 and tc["sem_classificacao"] == 1, v
    assert tc["capturadas_ids"] == [16, 25, 29, 32, 63], v
    assert sum(d["total"] for d in tc["dias"]) == 5, v
    # A semana zera exatamente no domingo 00h de Brasília.
    reset = sql("SELECT (extract(epoch FROM '2026-11-22 00:00:00-03'::timestamptz)*1000)::bigint")
    assert query(reset)["top_catch"]["especies"] == 0
    sql("TRUNCATE nexus.evento; INSERT INTO nexus.evento VALUES ('conta-a',null,'capturado','2026-11-18 11:30:00-03',decode('11','hex'),'{\"especie\":\"Abra\"}');")
    v = query()
    assert v["total"] == 1 and v["top_catch"]["projecao"] is None, v
    sql("TRUNCATE nexus.pokedex_motor")
    assert query()["top_catch"]["catalogo_disponivel"] is False
    print("OK: semana domingo/Brasília, reset, isolamento de conta, deduplicação, espécies únicas, elegibilidade, projeção limitada, amostra insuficiente e catálogo ausente.")
finally:
    subprocess.run(["runuser", "-u", "postgres", "--", pg_bin + "pg_ctl", "-D",
                    str(base / "data"), "-m", "immediate", "stop"], capture_output=True)
    shutil.rmtree(base)
