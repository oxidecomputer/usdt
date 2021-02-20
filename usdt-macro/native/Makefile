PROVIDER_FILE=provider.d
USDT_FILE=usdt.c
USDT_OBJECT=usdt.o
STATIC_LIB=libusdt.a
OUTPUT_DIR=../../target/native

ifneq ($(shell uname -s),Darwin)
	PROVIDER_OBJECT=$(OUTPUT_DIR)/provider.o
	DTRACE_COMPILE_COMMAND=dtrace -G -s \
		$(PROVIDER_FILE) \
		$(OUTPUT_DIR)/$(USDT_OBJECT) \
		-o $(PROVIDER_OBJECT)
else
	PROVIDER_OBJECT=
	DTRACE_COMPILE_COMMAND=
endif


lib: provider.h provider.o usdt.o
	ar crus $(OUTPUT_DIR)/$(STATIC_LIB) $(PROVIDER_OBJECT) $(OUTPUT_DIR)/$(USDT_OBJECT)

provider.h: $(PROVIDER_FILE)
	dtrace -h -s $(PROVIDER_FILE)

usdt.o: usdt.c usdt.h provider.h
	mkdir -p $(OUTPUT_DIR)
	cc -c -o $(OUTPUT_DIR)/$(USDT_OBJECT) -fPIC usdt.c

provider.o: usdt.o
	mkdir -p $(OUTPUT_DIR)
	$(DTRACE_COMPILE_COMMAND)

clean:
	rm -f provider.h
