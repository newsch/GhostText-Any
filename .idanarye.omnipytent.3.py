import vim
from omnipytent import *
from omnipytent.execution import ShellCommandExecuter
from omnipytent.integration.plumbum import local

VAR['$RUST_LOG'] = 'ghost_text_file=info'


@ShellCommandExecuter
def ERUN(cmd):
    CMD.Erun.bang(cmd)


cargo = local['cargo']


@task
def compile(ctx):
    cargo['build', '-q'] & ERUN


@task
def run(ctx):
    cargo['run', '-q'] & TERMINAL_PANEL

