CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY, /* GATE */
    label TEXT,
    password TEXT,
    enabled INTEGER DEFAULT 1,
    created INTEGER
);
CREATE TABLE IF NOT EXISTS sessions (
    hash TEXT PRIMARY KEY, /* GATE */
    account TEXT REFERENCES accounts(id), /* GATE */
    device TEXT,
    expires INTEGER
);
CREATE TABLE IF NOT EXISTS devices (
    account TEXT REFERENCES accounts(id), /* GATE */
    id TEXT,
    name TEXT,
    os TEXT,
    arch TEXT,
    first_seen INTEGER,
    last_seen INTEGER,
    PRIMARY KEY(account,id) /* GATE */
);
CREATE TABLE IF NOT EXISTS products (
    id TEXT PRIMARY KEY, /* GATE */
    label TEXT,
    description TEXT
);
CREATE TABLE IF NOT EXISTS grants (
    account TEXT REFERENCES accounts(id), /* GATE */
    product TEXT REFERENCES products(id), /* GATE */
    device TEXT DEFAULT '*',
    PRIMARY KEY(account,product,device) /* GATE */
);
CREATE TABLE IF NOT EXISTS selections (
    account TEXT REFERENCES accounts(id), /* GATE */
    product TEXT REFERENCES products(id), /* GATE */
    device TEXT,
    created INTEGER,
    PRIMARY KEY(account,product,device) /* GATE */
);
CREATE TABLE IF NOT EXISTS releases (
    product TEXT REFERENCES products(id), /* GATE */
    target TEXT,
    sequence INTEGER,
    sha256 TEXT,
    version TEXT,
    body TEXT,
    PRIMARY KEY(product,target,sequence) /* GATE */
);
CREATE TABLE IF NOT EXISTS deployments (
    id TEXT PRIMARY KEY, /* GATE */
    account TEXT,
    device TEXT,
    product TEXT,
    sequence INTEGER,
    version TEXT,
    status TEXT,
    detail TEXT DEFAULT '{}',
    created INTEGER,
    updated INTEGER,
    UNIQUE(account,device,product,sequence), /* GATE */
    FOREIGN KEY(account,device) REFERENCES devices(account,id) /* GATE */
);
CREATE TABLE IF NOT EXISTS clients (
    target TEXT PRIMARY KEY, /* GATE */
    sha256 TEXT,
    size INTEGER,
    version TEXT
);
