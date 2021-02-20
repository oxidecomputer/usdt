#include "usdt.h"
#include "provider.h"
#include <stdio.h>

void emit0(const char* probe_name) {
	printf("C: probe \"%s\"\n", probe_name);
	USDT_EMIT0(probe_name);
}
