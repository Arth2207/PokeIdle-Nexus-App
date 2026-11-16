#!/usr/bin/env python3
"""Valida Outlands em PostgreSQL 16 descartável, sem TCP nem banco de produção.
Requer root e o usuário postgres: python3 ferramentas/fixtures/testar-outlands.py
"""
import json
import pathlib
import shutil
import subprocess
import tempfile

base = pathlib.Path(tempfile.mkdtemp(prefix="nexus-outlands-teste-"))
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
    import re
    pg(pg_bin + "initdb", "-D", str(base / "data"), "-A", "trust", "--no-locale", "--encoding=UTF8")
    pg(pg_bin + "pg_ctl", "-D", str(base / "data"), "-l", str(base / "log"),
       "-o", f"-k {base} -p 55439 -c listen_addresses=''", "-w", "start")
    repo = pathlib.Path(__file__).resolve().parents[2]
    sql("""CREATE SCHEMA nexus;
        CREATE TABLE nexus.evento(conta text, conta_id text, tipo text, em timestamptz, dados jsonb);
        CREATE TABLE nexus.motor_janela(conta text PRIMARY KEY, em timestamptz, janela jsonb, login jsonb);
        CREATE TABLE nexus.motor_config(nick text, secao text, dados jsonb, texto json);
        CREATE TABLE public.fato_venda_mercado(em bigint, tipo text, moeda text, preco numeric, quantidade integer, descricao text);
        INSERT INTO nexus.motor_config VALUES('Ana','hunt','{"objetivo":"xp"}',null);
    """)
    sql((repo / 'migrations/20261125000001_outlands.sql').read_text())
    def strings(path, function='query_scalar'):
        source = (repo / path).read_text()
        return [json.loads('"' + s.replace('\n', '\\n') + '"') for s in
                re.findall(r'sqlx::' + function + r'\(\s*"((?:\\.|[^"\\])*)"', source)]
    queries = strings('crates/persistencia/src/outlands.rs')
    assert len(queries) == 4, len(queries)
    def query(i, types='text', params="'a'"):
        out = sql(f'PREPARE q({types}) AS ' + queries[i] + f'; EXECUTE q({params});')
        return [json.loads(l) for l in out.splitlines() if l.startswith(('{', '['))]
    sql("""INSERT INTO nexus.motor_janela VALUES ('a',now(),'{"nick":"Ana","ultimo":{"outlandTier":104}}','{}');
        INSERT INTO nexus.evento VALUES ('ana','a','welcome',now()-interval '2 hours',
          '{"payload":{"outlandTiers":[{"tier":104,"numero":4,"nivel":1000,"mult":1.25,"rotulo":"Outland 4"}],"chanceBossToken":0.0001}}');
    """)
    ctx = query(0)[0]
    assert ctx['regras']['chanceBossToken'] == 0.0001, ctx
    assert ctx['regras']['outlandTiers'][0]['numero'] == 4, ctx
    contexto = {'outland_tier':104,'regras_id':'nova','ativo_id':1,'ativo_nivel':1000,'loot':None,'hunt_slug':'furious_scizor'}
    def evento(tipo, segundos, dados):
        text=json.dumps(dados, ensure_ascii=False).replace("'","''")
        sql(f"INSERT INTO nexus.evento VALUES('ana','a','{tipo}',now()-interval '1 hour'+interval '{segundos} seconds','{text}'::jsonb)")
    for t in [0,15,30,45]:
        evento('hunt.contexto', t, {'contexto':contexto})
    evento('kill',20,{'nome':'Furious Scizor','ouro':10,'contexto':contexto})
    evento('drop',21,{'alvo_especie':'Furious Scizor','item_nome':'Bronze Boss Token','item_id':1,'quantidade':2,'contexto':contexto})
    antigo=dict(contexto,regras_id='antiga')
    evento('kill',22,{'nome':'Furious Scizor','contexto':antigo})
    evento('kill',23,{'nome':'Furious Scizor'})
    periodos=query(2)
    assert len(periodos)==1 and abs(periodos[0]['horas']-45/3600)<0.0001, periodos
    linhas=query(1)
    assert len(linhas)==4, linhas
    assert sum(l['qtd'] for l in linhas if l['tipo']=='drop')==2, linhas
    assert len([l for l in linhas if l['tipo']=='kill' and l['tier'] is None])==1,linhas
    # Uma mudança de contexto não recebe o intervalo da região anterior.
    evento('hunt.contexto',60,{'contexto':dict(contexto,outland_tier=105)})
    periodos=query(2)
    assert all(p['horas']==0 for p in periodos if p['tier']=='105'),periodos
    sql("INSERT INTO public.fato_venda_mercado VALUES (1799999990000,'item','orb',40,2,'Bronze Boss Token'),(1799999990001,'item','orb',40,2,'Bronze Boss Token'),(1799999990002,'item','orb',40,2,'Bronze Boss Token'),(1799999990003,'item','orb',999,0,'Bronze Boss Token')")
    cotacao=sql("PREPARE cotacao(bigint,bigint) AS "+queries[3]+"; EXECUTE cotacao(1799395200000,1800000000000);")
    assert float(cotacao.splitlines()[-1])==20,cotacao
    # Testa o SQL exato da gravação atômica do objetivo.
    updates=[q for q in strings('crates/persistencia/src/motor.rs','query') if q.startswith('UPDATE nexus.motor_config')]
    assert len(updates)==1
    sql("PREPARE objetivo(text,text,text,jsonb) AS "+updates[0]+"; EXECUTE objetivo('Ana','hunt','objetivo','\"gemas\"');")
    dados=json.loads(sql("SELECT dados FROM nexus.motor_config WHERE nick='Ana'"))
    assert dados['objetivo']=='gemas' and dados['objetivoManual'] is True,dados
    print('OK: catálogo bruto como fallback, ID versus número, agrupamento por contexto/versão, intervalos observados, transições e objetivo atômico.')
finally:
    subprocess.run(["runuser", "-u", "postgres", "--", pg_bin + "pg_ctl", "-D",
                    str(base / "data"), "-m", "immediate", "stop"], capture_output=True)
    shutil.rmtree(base)
