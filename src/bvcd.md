
+-----------------------+
|                       |
|   Header (unspecced)  |
|                       |
+-----------------------+

+---------------------------------- Header ----------------------------------+
|       [timescale] [date] [comment length] [utf8 comment]


+------ Number of Signals -------+
|          varint, u32           +---------------------------------+
+--------------------------------+                                 |
                                                                   ∨
+---------------------------------------- The Signal structure [repeated] --------------------------------------+
|                                                                                                               |
|                                                    +-------------------------------------------------+        |
|                                                    |    struct {                                     |        |
|                                                    |        /// varint, u32                          |        |
|                                                    |        type: enum {                             |        |
|                                                    |            SINGLE_VALUE = 0,                    |        |
|                                                    |            VALUE_VECTOR = 1,                    |        |
|                                                    |            ASCII = 2,                           |        |
|                                                    |        },                                       |        |
|                                                    |        /// The metadata is only present         |        |
|                                                    |        /// if the type requires it.             |        |
|                                                    |        /// The size of the metadata             |        |
|                                                    |        /// is dependent on the type.            |        |
|                                                    |        metadata:                                |        |
|                                                    |            if type ∈ { VALUE_VECTOR, ASCII }    |        |
|                                                    |                number_of_qits: varint, u32      |        |
|                                                    |    }                                            |        |
|                                                    +-------------------------------------------------+        |
|                                                                         |                                     |
|                                       +-------------+                   |                                     |
|                                       | varint, u32 |                   |                                     |
|                                       +-------------+                   |                                     |
|                                              |                          |                                     |
|                          [signal id] [name length] [utf8 name] [type + metadata]                              |
|                                 |                         |                                                   |
|        +--------------------------------------------+     |                                                   |
|        | unsigned varint (up to u64::MAX - 1)       |     |                                                   |
|        | must be sequential (e.g. 0, 1, 2, 3, ...)  |     |                                                   |
|        +--------------------------------------------+     |                                                   |
|                                                           |                                                   |
|                                                           |                                                   |
|                                    +-----------------------------------------------+                          |
|                                    | sequence of [name length] utf-8 encoded bytes |                          |
|                                    +-----------------------------------------------+                          |
|                                                                                                               |
+---------------------------------------------------------------------------------------------------------------+


                +----------------------------------------------------------------------------------------------------------------------+
                |                                                                                                                      |
                ∨                                                                                                                      |
+------ Number of Scopes --------+                                                                                                     |
|          varint, u32           +---------------------------------------+                                                             |
+--------------------------------+                                       |                                                             |
                                                                         ∨                                                             |
+---------------------------------------------------------- A Scope  [repeated] ----------------------------------------------+        |
|                                                                                                                             |        |
|    +------------------- The Scope Metadata structure [repeated] ----------------+                                           |        |
|    |                                                                            |                                           |        |
|    |                     +-------------+                                        |                                           |        |
|    |                     | varint, u32 |                                        |                                           |        |
|    |                     +-------------+                                        |                                           |        |
|    |                            |                                               |                                           |        |
|    |                       [name length] [utf8 name]                            |                                           |        |
|    |                                          |                                 |                                           |        |
|    |                                          |                                 |                                           |        |
|    |              +-----------------------------------------------+             |                                           |        |
|    |              | sequence of [name length] utf-8 encoded bytes |             |                                           |        |
|    |              +-----------------------------------------------+             |                                           |        |
|    |                                                                            |                                           |        |
|    +----------------------------------------------------------------------------+                                           |        |
|                                                                                                                             |        |
|    +----- Number of Variables in current scope -----+                                                                       |        |
|    |                   varint, u32                  +-------------------+                                                   |        |
|    +------------------------------------------------+                   |                                                   |        |
|                                                                         ∨                                                   |        |
|    +---------------------------------------- The Variable structure [repeated] ----------------------------------------+    |        |
|    |                                                                                                                   |    |        |
|    |                          +--------------------------------------------+                                           |    |        |
|    |                          | unsigned varint (up to u64::MAX - 1)       |                                           |    |        |
|    |                          | must be sequential (e.g. 0, 1, 2, 3, ...)  |                                           |    |        |
|    |                          +--------------------------------------------+                                           |    |        |
|    |                                                        |                                                          |    |        |
|    |                       +-------------+                  |                                                          |    |        |
|    |                       | varint, u32 |                  |                                                          |    |        |
|    |                       +-------------+                  |                                                          |    |        |
|    |                               |                        |                                                          |    |        |
|    |                         [name length] [utf8 name] [signal id] [start of bit range] [end of bit range]             |    |        |
|    |                                             |                          |                    |                     |    |        |
|    |                                             |                          |                    |                     |    |        |
|    |                                             |                          |                    |                     |    |        |
|    |          +-----------------------------------------------+             +----------+---------+                     |    |        |
|    |          | sequence of [name length] utf-8 encoded bytes |                        |                               |    |        |
|    |          +-----------------------------------------------+                        |                               |    |        |
|    |                                                                                   |                               |    |        |
|    |                                                +-------------------------------------------------------------+    |    |        |
|    |                                                | unsigned varint, u32                                        |    |    |        |
|    |                                                | must fit within bounds of size specified in signal metadata |    |    |        |
|    |                                                +-------------------------------------------------------------+    |    |        |
|    |                                                                                                                   |    |        |
|    +-------------------------------------------------------------------------------------------------------------------+    |        |
|                                                            |                                                                |        |
|                                                            |                                                                |        |
|                                                            +----------------------------------------------------------------+--------+
|                                                                                                                             |
+-----------------------------------------------------------------------------------------------------------------------------+

+---------------------------- Initial Signal Value (repeated [Number of Signals] times) ----------------------------+
|                                                                                                                   |
|               +---------------------------------------- Note ---------------------------------------+             |
|               | Since signal ids are sequential, these values are assigned to the respective signal |             |
|               +-------------------------------------------------------------------------------------+             |
|                                                                                                                   |
|                                                   [vector of qits]                                                |
|                                                          |                                                        |
|                                                          |                                                        |
|                              +----------------------------------------------------------+                         |
|                              | qit ∈ { x, z, 0, 1 }                                     |                         |
|                              |                                                          |                         |
|                              | four qits to a byte (padded up to a byte), little endian |                         |
|                              |     x => 0b00                                            |                         |
|                              |     z => 0b01                                            |                         |
|                              |     0 => 0b10                                            |                         |
|                              |     1 => 0b11                                            |                         |
|                              | e.g. x1 => 0b0011                                        |                         |
|                              |                                                          |                         |
|                              | if signal.type ∈ { VALUE_VECTOR, ASCII }                 |                         |
|                              |     vector contains metadata.number_of_qits              |                         |
|                              | if signal.type == SINGLE_VALUE                           |                         |
|                              |     contains a single qit in the least significant bits  |                         |
|                              +----------------------------------------------------------+                         |
|                                                                                                                   |
+-------------------------------------------------------------------------------------------------------------------+


===== Value Changes =====
The value changes section contains a list of delta-timestamped
+-----------------------+
|                       |
| List of the following structure
|  for each value change in timestamp 0
| - 
|                       |
+-----------------------+