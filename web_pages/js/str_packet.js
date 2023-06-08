class StrPacket {
    constructor(type, header = "||-=-||", type_sep = "-=-", arg_sep = "|.|") {
        this.type = type;
        this.header = header;
        this.type_sep = type_sep;
        this.arg_sep = arg_sep;
        this.args = "";
    }

    addArgs(...args) {
        for (const arg of args) {
            if (this.args.length === 0) {
                this.args += arg;
            } else {
                this.args += this.arg_sep + arg;
            }
        }
        return this;
    }

    to_str() {
        return this.header + this.type + this.type_sep + this.args;
    }

    from_str(str) {
        if (!str.startsWith(this.header)) {
            return null;
        }
        str = str.replace(this.header, "");
        const splitted_str = str.split(this.type_sep);
        if (splitted_str.length != 2) {
            return null;
        }
        let [type, data] = splitted_str;
        this.args = data.split(this.arg_sep);

        this.type = type;
        return [this.type, this.args];
    }
}