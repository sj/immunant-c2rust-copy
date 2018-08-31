'''This module generates `GetNodeId` impls for each AST node type that has a
`NodeId`.

- Structs with a field named `id` or a field with the `#[node_id]` attribute
  get a `GetNodeId` impl that returns that ID.
- Enums and flags do not have IDs and thus do not get impls.

Attributes:

- `#[node_id]`: On a struct field, causes the field to be treated as the
  struct's node ID, even if its name is not `id`.  Only one field in a given
  struct can have this attribute.

- `#[no_node_id]`: On a struct, causes no impl to be generated, even if the
  struct has a field named `id`.
'''

from datetime import datetime
from textwrap import indent, dedent

from ast import *
from util import *


@linewise
def do_impl(s, field_name):
    yield 'impl GetNodeId for %s {' % s.name
    yield '  fn get_node_id(&self) -> NodeId {'
    yield '    self.%s' % field_name
    yield '  }'
    yield '}'

def find_id_field(s):
    if 'no_node_id' in s.attrs:
        return None

    marked_fields = []
    for f in s.fields:
        if 'node_id' in f.attrs:
            marked_fields.append(f.name)
    if len(marked_fields) == 1:
        return marked_fields[0]
    elif len(marked_fields) > 1:
        raise ValueError('struct %s has %d fields marked #[node_id] (expected 0 or 1)' %
                (s.name, len(marked_fields)))

    for f in s.fields:
        if f.name == 'id':
            return f.name

    return None

@linewise
def generate(decls):
    yield '// AUTOMATICALLY GENERATED - DO NOT EDIT'
    yield '// Produced %s by process_ast.py' % (datetime.now(),)
    yield ''

    for d in decls:
        if not isinstance(d, Struct):
            continue
        field_name = find_id_field(d)
        if field_name is not None:
            yield do_impl(d, field_name)
