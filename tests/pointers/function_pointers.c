typedef int char_to_int(char);
typedef int (*char_to_int_fp)(char);

int intval(const char c) { return c; }
int negintval(const char c) { return -c; }

void entry3(const unsigned sz, int buffer[const])
{
        int i = 0;

        char_to_int *p0;
        char_to_int *p1 = 0;
        char_to_int *p2 = intval;
        char_to_int *p3 = &intval;
        p3 = intval;
        p3 = &intval;

        if (!p1 && p3 && !!p3 && p3 != 0) {
                buffer[i] = p3('a');
        }
        i++;

        char_to_int_fp p4;
        char_to_int_fp p5 = 0;
        char_to_int_fp p6 = intval;
        char_to_int_fp p7 = &intval;
        p7 = intval;
        p7 = &intval;

        if (!p5 && p7 && !!p7 && p7 != 0) {
                buffer[i] = (*p7)('a');
        }
        i++;

        char_to_int_fp funs[3] = { intval, negintval };

        for (int j = 0; funs[j]; j++) {
                buffer[i++] = funs[j] ? funs[j]('a'+j) : 55;
        }

        // validate function pointer comparison to null
        buffer[i++] = p1 == 0;
        buffer[i++] = p1 != 0;
        buffer[i++] = p2 == 0;
        buffer[i++] = p2 != 0;

        // These should now use is_some, is_none:
        int j = p4 == 0;
        j = 0 == p4;
        j = p4 != 0;
        j = 0 != p4;
}
