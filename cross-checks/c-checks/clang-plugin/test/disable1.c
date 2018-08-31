// RUN: %clang_xcheck -O2 -o %t %s %xcheck_runtime %fakechecks
// RUN: %t 2>&1 | FileCheck %s

#include <stdio.h>

#include <cross_checks.h>

int foo() DISABLE_XCHECKS(true) {
    return 1;
}

int main() {
    foo();
    return 0;
}
// CHECK: XCHECK(Ent):2090499946/0x7c9a7f6a
// CHECK-NOT: XCHECK(Ent):193491849/0x0b887389
// CHECK-NOT: XCHECK(Exi):193491849/0x0b887389
// CHECK: XCHECK(Exi):2090499946/0x7c9a7f6a
// CHECK: XCHECK(Ret):8680820740569200758/0x7878787878787876
